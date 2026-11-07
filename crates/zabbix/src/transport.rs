// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::gateway::{ReadMethod, SessionMethod, WriteGrant, WriteMethod};
use crate::{Result, ZabbixError};
use futures_util::StreamExt;
use reqwest::{Certificate, Client, redirect::Policy};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fmt::{self, Write as _},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use url::Url;

const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
const API_FILE: &str = "api_jsonrpc.php";

/// Why an address was rejected; wire values are the `reason` parameter of
/// `invalid_address`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EndpointRejection {
    Syntax,
    Scheme,
    Userinfo,
    Query,
    Fragment,
    MissingHost,
}

impl EndpointRejection {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::Scheme => "scheme",
            Self::Userinfo => "userinfo",
            Self::Query => "query",
            Self::Fragment => "fragment",
            Self::MissingHost => "host",
        }
    }
}

/// Zabbix front-end base URL and the API URL derived from it by URL
/// resolution, never by string concatenation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZabbixEndpoint {
    base_url: Url,
    api_url: Url,
}

impl ZabbixEndpoint {
    /// Accepts the address as shown in a browser: `http`/`https`, a name,
    /// IPv4 or bracketed IPv6 host, an optional port and base path, with or
    /// without the trailing `/` or `api_jsonrpc.php`. The scheme and host are
    /// lower-cased and a default port is dropped by the URL parser.
    pub fn parse(value: &str) -> Result<Self> {
        let url = Url::parse(value.trim())
            .map_err(|_| ZabbixError::InvalidEndpoint(EndpointRejection::Syntax))?;
        let rejection = if !matches!(url.scheme(), "http" | "https") {
            Some(EndpointRejection::Scheme)
        } else if !url.username().is_empty() || url.password().is_some() {
            Some(EndpointRejection::Userinfo)
        } else if url.query().is_some() {
            Some(EndpointRejection::Query)
        } else if url.fragment().is_some() {
            Some(EndpointRejection::Fragment)
        } else if url.host_str().is_none_or(str::is_empty) {
            Some(EndpointRejection::MissingHost)
        } else {
            None
        };
        if let Some(rejection) = rejection {
            return Err(ZabbixError::InvalidEndpoint(rejection));
        }
        let mut base_url = url;
        let mut path = base_url.path().trim_end_matches('/').to_owned();
        if let Some(base) = path.strip_suffix(&format!("/{API_FILE}")) {
            path = base.trim_end_matches('/').to_owned();
        }
        path.push('/');
        base_url.set_path(&path);
        let api_url = base_url
            .join(API_FILE)
            .map_err(|_| ZabbixError::InvalidEndpoint(EndpointRejection::Syntax))?;
        Ok(Self { base_url, api_url })
    }

    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    pub fn api_url(&self) -> &Url {
        &self.api_url
    }

    /// Credentials sent to this endpoint travel in clear text.
    #[must_use]
    pub fn is_plain_http(&self) -> bool {
        self.base_url.scheme() == "http"
    }

    /// Stable fingerprint of the normalized API URL.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        hex_sha256(&[b"zdashstitch-endpoint-v1", self.api_url.as_str().as_bytes()])
    }
}

/// Why trusted CA material was rejected; wire values are the `reason`
/// parameter of `trust_material_invalid`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrustMaterialError {
    TooLarge,
    Empty,
    InvalidPem,
    PrivateKey,
    TooManyCertificates,
    Rejected,
}

impl TrustMaterialError {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TooLarge => "too_large",
            Self::Empty => "empty",
            Self::InvalidPem => "invalid_pem",
            Self::PrivateKey => "private_key",
            Self::TooManyCertificates => "too_many_certificates",
            Self::Rejected => "rejected",
        }
    }
}

const MAX_TRUST_PEM_BYTES: usize = 256 * 1024;
const MAX_TRUST_CERTIFICATES: usize = 32;

/// Trust anchors for TLS: always the operating-system store, plus explicitly
/// added CA certificates. There is deliberately no way to disable or weaken
/// certificate verification.
#[derive(Clone, Default)]
pub struct TrustAnchors {
    roots: Vec<Vec<u8>>,
}

impl TrustAnchors {
    /// Operating-system trust store only.
    #[must_use]
    pub fn system() -> Self {
        Self::default()
    }

    /// System store plus every certificate of a PEM bundle. The bundle may
    /// contain only `CERTIFICATE` blocks and free text between them; private
    /// keys and other block types are rejected.
    pub fn with_pem_bundle(pem: &[u8]) -> Result<Self> {
        let roots = parse_certificate_pem(pem).map_err(ZabbixError::TrustMaterial)?;
        let anchors = Self { roots };
        // The TLS stack must accept every root before it is stored.
        anchors
            .client_builder()
            .map_err(ZabbixError::TrustMaterial)?
            .build()
            .map_err(|_| ZabbixError::TrustMaterial(TrustMaterialError::Rejected))?;
        Ok(anchors)
    }

    #[must_use]
    pub fn is_system_only(&self) -> bool {
        self.roots.is_empty()
    }

    /// SHA-256 of each added certificate (DER), lower-case hex, the same
    /// value `openssl x509 -fingerprint -sha256` shows.
    #[must_use]
    pub fn certificate_fingerprints(&self) -> Vec<String> {
        self.roots.iter().map(|der| plain_sha256(der)).collect()
    }

    /// Order-independent fingerprint of the added material; `None` for the
    /// system store only.
    #[must_use]
    pub fn fingerprint(&self) -> Option<String> {
        if self.roots.is_empty() {
            return None;
        }
        let mut fingerprints = self.certificate_fingerprints();
        fingerprints.sort();
        fingerprints.dedup();
        let joined = fingerprints.join(",");
        Some(hex_sha256(&[b"zdashstitch-trust-v1", joined.as_bytes()]))
    }

    /// Canonical PEM of the added certificates, for local storage.
    #[must_use]
    pub fn to_pem(&self) -> String {
        let mut pem = String::new();
        for der in &self.roots {
            pem.push_str("-----BEGIN CERTIFICATE-----\n");
            let encoded = base64_encode(der);
            for chunk in encoded.as_bytes().chunks(64) {
                pem.push_str(&String::from_utf8_lossy(chunk));
                pem.push('\n');
            }
            pem.push_str("-----END CERTIFICATE-----\n");
        }
        pem
    }

    fn client_builder(&self) -> std::result::Result<reqwest::ClientBuilder, TrustMaterialError> {
        let builder = Client::builder()
            .redirect(Policy::none())
            .timeout(Duration::from_secs(30));
        if self.roots.is_empty() {
            return Ok(builder);
        }
        let certificates = self
            .roots
            .iter()
            .map(|der| Certificate::from_der(der).map_err(|_| TrustMaterialError::Rejected))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(builder.tls_certs_merge(certificates))
    }
}

impl fmt::Debug for TrustAnchors {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TrustAnchors")
            .field("custom_roots", &self.roots.len())
            .finish()
    }
}

pub(crate) struct Transport {
    endpoint: ZabbixEndpoint,
    client: Client,
    next_id: AtomicU64,
}

impl Transport {
    pub(crate) fn new(endpoint: ZabbixEndpoint, trust: &TrustAnchors) -> Result<Self> {
        let client = trust
            .client_builder()
            .map_err(ZabbixError::TrustMaterial)?
            .build()
            .map_err(ZabbixError::Client)?;
        Ok(Self {
            endpoint,
            client,
            next_id: AtomicU64::new(1),
        })
    }

    pub(crate) fn endpoint(&self) -> &ZabbixEndpoint {
        &self.endpoint
    }

    /// Session methods. `user.checkAuthentication` must be sent without the
    /// authorization header (Zabbix 7.4 rejects it otherwise).
    pub(crate) async fn session<T: DeserializeOwned>(
        &self,
        method: SessionMethod,
        params: Value,
        token: Option<&SecretString>,
    ) -> Result<T> {
        let token = match method {
            SessionMethod::ApiInfoVersion | SessionMethod::UserCheckAuthentication => None,
            SessionMethod::UserLogin | SessionMethod::UserLogout => token,
        };
        self.call(method.api_method(), params, token).await
    }

    /// Allowlisted read methods (P1-IN-08).
    pub(crate) async fn read<T: DeserializeOwned>(
        &self,
        method: ReadMethod,
        params: Value,
        token: &SecretString,
    ) -> Result<T> {
        self.call(method.api_method(), params, Some(token)).await
    }

    /// The role ID comes from a fresh check of this credential in the
    /// gateway. Keep the scope and selected fields out of generic reads.
    pub(crate) async fn read_own_role_rules(
        &self,
        roleid: &str,
        token: &SecretString,
    ) -> Result<Vec<Value>> {
        self.call(
            crate::create_permission::OWN_ROLE_METHOD,
            serde_json::json!({
                "roleids": [roleid],
                "output": ["roleid", "type"],
                "selectRules": ["api.access", "api.mode", "api", "actions"]
            }),
            Some(token),
        )
        .await
    }

    /// Write methods require a grant that only `ReadWriteGateway` holds.
    pub(crate) async fn write<T: DeserializeOwned>(
        &self,
        method: WriteMethod,
        params: Value,
        token: &SecretString,
        _grant: &WriteGrant,
    ) -> Result<T> {
        self.call(method.api_method(), params, Some(token)).await
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: &'static str,
        params: Value,
        token: Option<&SecretString>,
    ) -> Result<T> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = RpcRequest {
            jsonrpc: "2.0",
            method,
            params,
            id,
        };
        let mut builder = self
            .client
            .post(self.endpoint.api_url().clone())
            .json(&request);
        if let Some(token) = token {
            builder = builder.bearer_auth(token.expose_secret());
        }
        let response = builder.send().await.map_err(ZabbixError::Transport)?;
        if !response.status().is_success() {
            return Err(ZabbixError::HttpStatus(response.status()));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(ZabbixError::ResponseTooLarge {
                limit: MAX_RESPONSE_BYTES,
            });
        }
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(ZabbixError::Transport)?;
            if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(ZabbixError::ResponseTooLarge {
                    limit: MAX_RESPONSE_BYTES,
                });
            }
            bytes.extend_from_slice(&chunk);
        }
        let response: RpcResponse<T> =
            serde_json::from_slice(&bytes).map_err(ZabbixError::InvalidResponse)?;
        if response.id != id {
            return Err(ZabbixError::ResponseIdMismatch);
        }
        match (response.result, response.error) {
            (Some(result), None) => Ok(result),
            (None, Some(error)) => Err(ZabbixError::Api {
                method: method.to_owned(),
                code: error.code,
                message: error.message,
                data: error.data,
            }),
            _ => Err(ZabbixError::InvalidResponseShape),
        }
    }

    pub(crate) fn endpoint_identity(&self) -> &str {
        self.endpoint.api_url().as_str()
    }
}

#[derive(Serialize)]
struct RpcRequest<'a> {
    jsonrpc: &'static str,
    method: &'a str,
    params: Value,
    id: u64,
}

#[derive(Deserialize)]
struct RpcResponse<T> {
    id: u64,
    result: Option<T>,
    error: Option<RpcError>,
}

#[derive(Deserialize)]
struct RpcError {
    code: i64,
    message: String,
    #[serde(default)]
    data: Option<String>,
}

fn plain_sha256(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

/// Domain-separated SHA-256 over length-prefixed parts, lower-case hex.
pub(crate) fn hex_sha256(parts: &[&[u8]]) -> String {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part);
    }
    let mut hex = String::with_capacity(64);
    for byte in digest.finalize() {
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

fn parse_certificate_pem(pem: &[u8]) -> std::result::Result<Vec<Vec<u8>>, TrustMaterialError> {
    if pem.len() > MAX_TRUST_PEM_BYTES {
        return Err(TrustMaterialError::TooLarge);
    }
    let text = std::str::from_utf8(pem).map_err(|_| TrustMaterialError::InvalidPem)?;
    let mut roots = Vec::new();
    let mut block: Option<(String, String)> = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(label) = line
            .strip_prefix("-----BEGIN ")
            .and_then(|rest| rest.strip_suffix("-----"))
        {
            if block.is_some() {
                return Err(TrustMaterialError::InvalidPem);
            }
            if label.contains("PRIVATE KEY") {
                return Err(TrustMaterialError::PrivateKey);
            }
            block = Some((label.to_owned(), String::new()));
        } else if let Some(label) = line
            .strip_prefix("-----END ")
            .and_then(|rest| rest.strip_suffix("-----"))
        {
            let Some((open, body)) = block.take() else {
                return Err(TrustMaterialError::InvalidPem);
            };
            if open != label || open != "CERTIFICATE" {
                return Err(TrustMaterialError::InvalidPem);
            }
            let der = base64_decode(&body).ok_or(TrustMaterialError::InvalidPem)?;
            if !is_der_sequence(&der) {
                return Err(TrustMaterialError::InvalidPem);
            }
            if !roots.contains(&der) {
                roots.push(der);
            }
            if roots.len() > MAX_TRUST_CERTIFICATES {
                return Err(TrustMaterialError::TooManyCertificates);
            }
        } else if let Some((_, body)) = block.as_mut() {
            body.push_str(line);
        }
    }
    if block.is_some() {
        return Err(TrustMaterialError::InvalidPem);
    }
    if roots.is_empty() {
        return Err(TrustMaterialError::Empty);
    }
    Ok(roots)
}

/// Outer DER `SEQUENCE` whose definite length covers exactly the input.
fn is_der_sequence(der: &[u8]) -> bool {
    let (Some(&0x30), Some(&first)) = (der.first(), der.get(1)) else {
        return false;
    };
    let (length, header) = if first < 0x80 {
        (usize::from(first), 2)
    } else {
        let count = usize::from(first & 0x7f);
        if count == 0 || count > 4 || der.len() < 2 + count {
            return false;
        }
        let length = der[2..2 + count]
            .iter()
            .fold(0usize, |length, byte| (length << 8) | usize::from(*byte));
        (length, 2 + count)
    };
    header + length == der.len()
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let symbols: Vec<u8> = text
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    if symbols.is_empty() || !symbols.len().is_multiple_of(4) {
        return None;
    }
    let padding = symbols
        .iter()
        .rev()
        .take_while(|byte| **byte == b'=')
        .count();
    if padding > 2 {
        return None;
    }
    let mut output = Vec::with_capacity(symbols.len() / 4 * 3);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for (index, symbol) in symbols.iter().enumerate() {
        if *symbol == b'=' {
            if index < symbols.len() - padding {
                return None;
            }
            continue;
        }
        let value = BASE64.iter().position(|candidate| candidate == symbol)?;
        buffer = (buffer << 6) | u32::try_from(value).ok()?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push(u8::try_from((buffer >> bits) & 0xff).ok()?);
        }
    }
    Some(output)
}

fn base64_encode(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let triple = chunk.iter().enumerate().fold(0u32, |value, (index, byte)| {
            value | (u32::from(*byte) << (16 - 8 * index))
        });
        for index in 0..4 {
            if index <= chunk.len() {
                let symbol = (triple >> (18 - 6 * index)) & 0x3f;
                output.push(char::from(BASE64[symbol as usize]));
            } else {
                output.push('=');
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips_all_padding_lengths() {
        for length in 0..10usize {
            let bytes: Vec<u8> = (0..length).map(|index| (index * 37 + 11) as u8).collect();
            let encoded = base64_encode(&bytes);
            if length == 0 {
                assert!(encoded.is_empty());
                continue;
            }
            assert_eq!(base64_decode(&encoded), Some(bytes));
        }
        assert_eq!(base64_decode("TWFu"), Some(b"Man".to_vec()));
        assert_eq!(base64_decode("TQ=="), Some(b"M".to_vec()));
        assert!(base64_decode("T=Q=").is_none());
        assert!(base64_decode("TWF").is_none());
        assert!(base64_decode("TW!u").is_none());
    }

    #[test]
    fn pem_parser_rejects_keys_foreign_blocks_and_garbage() {
        assert_eq!(
            parse_certificate_pem(
                b"-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----\n"
            ),
            Err(TrustMaterialError::PrivateKey)
        );
        assert_eq!(
            parse_certificate_pem(
                b"-----BEGIN RSA PRIVATE KEY-----\nAAAA\n-----END RSA PRIVATE KEY-----\n"
            ),
            Err(TrustMaterialError::PrivateKey)
        );
        assert_eq!(
            parse_certificate_pem(b"-----BEGIN X509 CRL-----\nMAA=\n-----END X509 CRL-----\n"),
            Err(TrustMaterialError::InvalidPem)
        );
        assert_eq!(
            parse_certificate_pem(b"no certificates here"),
            Err(TrustMaterialError::Empty)
        );
        assert_eq!(
            parse_certificate_pem(
                b"-----BEGIN CERTIFICATE-----\n!!!!\n-----END CERTIFICATE-----\n"
            ),
            Err(TrustMaterialError::InvalidPem)
        );
        assert_eq!(
            parse_certificate_pem(b"-----BEGIN CERTIFICATE-----\nMAA=\n"),
            Err(TrustMaterialError::InvalidPem)
        );
        assert_eq!(
            parse_certificate_pem(&vec![b' '; MAX_TRUST_PEM_BYTES + 1]),
            Err(TrustMaterialError::TooLarge)
        );
        // Minimal DER SEQUENCE, accepted structurally by the parser.
        assert_eq!(
            parse_certificate_pem(
                b"subject=example\n-----BEGIN CERTIFICATE-----\nMAA=\n-----END CERTIFICATE-----\n"
            ),
            Ok(vec![vec![0x30, 0x00]])
        );
    }

    #[test]
    fn der_length_must_match_exactly() {
        assert!(is_der_sequence(&[0x30, 0x00]));
        assert!(is_der_sequence(&[0x30, 0x01, 0x05]));
        assert!(!is_der_sequence(&[0x30, 0x02, 0x05]));
        assert!(!is_der_sequence(&[0x31, 0x00]));
        let mut long = vec![0x30, 0x81, 0x80];
        long.extend(std::iter::repeat_n(0u8, 0x80));
        assert!(is_der_sequence(&long));
    }
}
