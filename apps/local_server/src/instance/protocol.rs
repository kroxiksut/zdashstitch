// SPDX-License-Identifier: MIT OR Apache-2.0

//! Wire format of the single-instance channel, version 2: one request line,
//! one response line, both UTF-8 JSON terminated by `\n`, then the server
//! closes the channel.
//!
//! - Request: `{"v":2,"req":"bootstrap","nonce":"<64 hex>"}` (at most
//!   [`MAX_REQUEST`] bytes); `nonce` is 256 fresh random bits of the client.
//! - Response: `{"v":2,"mac":"<64 hex>","url":"http://127.0.0.1:<port>/?bootstrap=<64 hex>"}`
//!   or `{"v":2,"mac":"<64 hex>","error":"<code>"}` (at most
//!   [`MAX_RESPONSE`] bytes).
//!
//! `mac` is HMAC-SHA256 keyed with the random `proof` of the endpoint file,
//! which only the same operating-system user can read, over the client's
//! nonce and the reply itself (see [`response_mac`]). The proof never
//! travels over the channel, a recorded response is useless for another
//! nonce, and the second launch trusts no response without a valid `mac`.
//!
//! The server writes nothing before it has read one complete request line
//! with this version and a well-formed nonce: a silent, partial, oversize,
//! malformed or other-version request (including version 1) is closed
//! without a single byte, because no answer to it could be authenticated.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use super::InstanceError;

pub(super) const VERSION: u64 = 2;
/// Longest accepted request line, newline included.
pub(super) const MAX_REQUEST: usize = 256;
/// Longest accepted response line, newline included.
pub(super) const MAX_RESPONSE: usize = 1024;
/// Longest accepted endpoint file.
pub(super) const MAX_ENDPOINT_FILE: usize = 1024;

pub(super) const MALFORMED: &str = "instance.request.malformed";
pub(super) const UNSUPPORTED_VERSION: &str = "instance.request.unsupported_version";
pub(super) const UNKNOWN_REQUEST: &str = "instance.request.unknown";
pub(super) const TOO_LARGE: &str = "instance.request.too_large";
pub(super) const TIMEOUT: &str = "instance.request.timeout";
pub(super) const BUSY: &str = "instance.busy";

/// Domain separation of the response MAC from any other use of the proof.
const MAC_LABEL: &[u8] = b"zdashstitch.instance.v2.response";
const HEX_LENGTH: usize = 64;
const BOOTSTRAP_CODE_LENGTH: usize = 64;
const MAX_ERROR_CODE: usize = 64;
const SHA256_BLOCK: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Request {
    Bootstrap,
}

/// A request line that carries this version and a well-formed nonce, so
/// its answer can be authenticated; `request` may still be refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ParsedRequest {
    pub(super) nonce: String,
    pub(super) request: Result<Request, &'static str>,
}

/// The request line of a client nonce, newline included.
pub(super) fn request_line(nonce: &str) -> Vec<u8> {
    let mut line = serde_json::json!({ "v": VERSION, "req": "bootstrap", "nonce": nonce })
        .to_string()
        .into_bytes();
    line.push(b'\n');
    line
}

/// Parses one request line (without its `\n`). `Err` means no answer can
/// be authenticated (not an object, another version, no valid nonce): the
/// server closes the channel silently and only logs the code.
pub(super) fn parse_request(line: &[u8]) -> Result<ParsedRequest, &'static str> {
    let value: Value = serde_json::from_slice(line).map_err(|_| MALFORMED)?;
    let object = value.as_object().ok_or(MALFORMED)?;
    match object.get("v").map(Value::as_u64) {
        Some(Some(VERSION)) => {}
        Some(Some(_)) => return Err(UNSUPPORTED_VERSION),
        Some(None) | None => return Err(MALFORMED),
    }
    let nonce = match object.get("nonce").and_then(Value::as_str) {
        Some(nonce) if is_hex64(nonce) => nonce.to_owned(),
        _ => return Err(MALFORMED),
    };
    let request = if object.len() != 3 {
        Err(MALFORMED)
    } else {
        match object.get("req").map(Value::as_str) {
            Some(Some("bootstrap")) => Ok(Request::Bootstrap),
            Some(Some(_)) => Err(UNKNOWN_REQUEST),
            Some(None) | None => Err(MALFORMED),
        }
    };
    Ok(ParsedRequest { nonce, request })
}

/// What the server answers to one request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Reply {
    Url(String),
    Error(&'static str),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireResponse {
    v: u64,
    mac: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

pub(super) fn encode_response(proof: &str, nonce: &str, reply: &Reply) -> Vec<u8> {
    let (mac, url, error) = match reply {
        Reply::Url(url) => (
            response_mac(proof, nonce, "url", url),
            Some(url.clone()),
            None,
        ),
        Reply::Error(code) => (
            response_mac(proof, nonce, "error", code),
            None,
            Some((*code).to_owned()),
        ),
    };
    let mut line = serde_json::to_vec(&WireResponse {
        v: VERSION,
        mac,
        url,
        error,
    })
    .unwrap_or_else(|_| b"{}".to_vec());
    line.push(b'\n');
    line
}

/// Parses one response line (without its `\n`) and checks `mac` against
/// the proof and the nonce of this request before anything of the content
/// is trusted. Only an exact loopback bootstrap URL is returned, so nothing
/// else reaches the console.
pub(super) fn parse_response(
    line: &[u8],
    proof: &str,
    nonce: &str,
) -> Result<String, InstanceError> {
    let response: WireResponse =
        serde_json::from_slice(line).map_err(|_| InstanceError::InvalidResponse)?;
    if response.v != VERSION {
        return Err(InstanceError::InvalidResponse);
    }
    let (kind, value) = match (&response.url, &response.error) {
        (Some(url), None) => ("url", url.as_str()),
        (None, Some(code)) => ("error", code.as_str()),
        _ => return Err(InstanceError::InvalidResponse),
    };
    let expected = response_mac(proof, nonce, kind, value);
    if !constant_time_eq(response.mac.as_bytes(), expected.as_bytes()) {
        return Err(InstanceError::Unauthenticated);
    }
    match (response.url, response.error) {
        (Some(url), None) if is_bootstrap_url(&url) => Ok(url),
        (None, Some(code)) if is_error_code(&code) => Err(InstanceError::Refused(code)),
        _ => Err(InstanceError::InvalidResponse),
    }
}

/// `HMAC-SHA256(proof, label ‖ 0 ‖ nonce ‖ 0 ‖ kind ‖ 0 ‖ value)` as
/// lowercase hex. The nonce has a fixed length and `kind` is `url` or
/// `error`, so the encoding is unambiguous; `value` is the last field.
pub(super) fn response_mac(proof: &str, nonce: &str, kind: &str, value: &str) -> String {
    hex(&hmac_sha256(
        proof.as_bytes(),
        &[
            MAC_LABEL,
            b"\0",
            nonce.as_bytes(),
            b"\0",
            kind.as_bytes(),
            b"\0",
            value.as_bytes(),
        ],
    ))
}

/// HMAC (RFC 2104) over SHA-256 of the `sha2` crate already in the
/// workspace; the `hmac` crate in the lock file belongs to another `digest`
/// generation than `sha2` 0.10 and is not a dependency of this crate.
/// Checked against the RFC 4231 test vectors.
fn hmac_sha256(key: &[u8], message: &[&[u8]]) -> [u8; 32] {
    let mut block = [0_u8; SHA256_BLOCK];
    if key.len() > SHA256_BLOCK {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(block.map(|byte| byte ^ 0x36));
    for part in message {
        inner.update(part);
    }
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(block.map(|byte| byte ^ 0x5c));
    outer.update(inner);
    outer.finalize().into()
}

pub(super) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// `http://127.0.0.1:<port>/?bootstrap=<64 lowercase hex>` and nothing else.
pub(super) fn is_bootstrap_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://127.0.0.1:") else {
        return false;
    };
    let Some((port, code)) = rest.split_once("/?bootstrap=") else {
        return false;
    };
    let port_valid = !port.is_empty()
        && port.len() <= 5
        && port.bytes().all(|byte| byte.is_ascii_digit())
        && port.parse::<u16>().is_ok_and(|port| port != 0);
    port_valid && code.len() == BOOTSTRAP_CODE_LENGTH && is_lower_hex(code)
}

fn is_error_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= MAX_ERROR_CODE
        && code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._".contains(&byte))
}

/// 64 lowercase hex digits: a proof or a nonce.
pub(super) fn is_hex64(value: &str) -> bool {
    value.len() == HEX_LENGTH && is_lower_hex(value)
}

fn is_lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0_u8, |difference, (left, right)| {
                difference | (left ^ right)
            })
            == 0
}

/// Content of the endpoint file `instance.json`.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EndpointRecord {
    pub(super) v: u64,
    pub(super) endpoint: String,
    pub(super) proof: String,
}

impl EndpointRecord {
    pub(super) fn encode(&self) -> Vec<u8> {
        let mut bytes = serde_json::to_vec(self).unwrap_or_default();
        bytes.push(b'\n');
        bytes
    }

    /// `None` for anything but a well-formed record of this version; the
    /// endpoint name itself is checked by the platform layer.
    pub(super) fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() > MAX_ENDPOINT_FILE {
            return None;
        }
        let record: Self = serde_json::from_slice(bytes).ok()?;
        (record.v == VERSION && is_hex64(&record.proof)).then_some(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROOF: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const NONCE: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
    const CODE: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

    fn without_newline(mut line: Vec<u8>) -> Vec<u8> {
        assert_eq!(line.pop(), Some(b'\n'));
        line
    }

    #[test]
    fn hmac_matches_the_rfc_4231_vectors() {
        // Test case 1: 20 bytes 0x0b, "Hi There".
        assert_eq!(
            hex(&hmac_sha256(&[0x0b; 20], &[b"Hi There"])),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        // Test case 2: "Jefe", the message split in two parts.
        assert_eq!(
            hex(&hmac_sha256(
                b"Jefe",
                &[b"what do ya want ", b"for nothing?"]
            )),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        // Test case 6: a 131-byte key is hashed first.
        assert_eq!(
            hex(&hmac_sha256(
                &[0xaa; 131],
                &[b"Test Using Larger Than Block-Size Key - Hash Key First"]
            )),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    #[test]
    fn the_request_line_is_a_valid_request() {
        let line = request_line(NONCE);
        assert!(line.len() <= MAX_REQUEST);
        assert_eq!(
            parse_request(&without_newline(line)),
            Ok(ParsedRequest {
                nonce: NONCE.to_owned(),
                request: Ok(Request::Bootstrap),
            })
        );
    }

    #[test]
    fn requests_without_an_authenticatable_answer_are_rejected() {
        let short = &NONCE[1..];
        let upper = NONCE.to_uppercase();
        for line in [
            String::new(),
            "nope".to_owned(),
            "{".to_owned(),
            "[1]".to_owned(),
            "\"bootstrap\"".to_owned(),
            format!("{{\"req\":\"bootstrap\",\"nonce\":\"{NONCE}\"}}"),
            format!("{{\"v\":\"2\",\"req\":\"bootstrap\",\"nonce\":\"{NONCE}\"}}"),
            format!("{{\"v\":2.5,\"req\":\"bootstrap\",\"nonce\":\"{NONCE}\"}}"),
            format!("{{\"v\":-1,\"req\":\"bootstrap\",\"nonce\":\"{NONCE}\"}}"),
            "{\"v\":2,\"req\":\"bootstrap\"}".to_owned(),
            format!("{{\"v\":2,\"req\":\"bootstrap\",\"nonce\":\"{short}\"}}"),
            format!("{{\"v\":2,\"req\":\"bootstrap\",\"nonce\":\"{upper}\"}}"),
            "{\"v\":2,\"req\":\"bootstrap\",\"nonce\":7}".to_owned(),
        ] {
            assert_eq!(parse_request(line.as_bytes()), Err(MALFORMED), "{line}");
        }
        assert_eq!(parse_request(b"\xff\xfe"), Err(MALFORMED));
    }

    #[test]
    fn version_1_and_other_versions_are_refused_without_an_answer() {
        for line in [
            "{\"v\":1,\"req\":\"bootstrap\"}".to_owned(),
            format!("{{\"v\":1,\"req\":\"bootstrap\",\"nonce\":\"{NONCE}\"}}"),
            format!("{{\"v\":3,\"req\":\"bootstrap\",\"nonce\":\"{NONCE}\"}}"),
            "{\"v\":0}".to_owned(),
        ] {
            assert_eq!(
                parse_request(line.as_bytes()),
                Err(UNSUPPORTED_VERSION),
                "{line}"
            );
        }
    }

    #[test]
    fn a_request_with_a_nonce_is_answered_even_when_refused() {
        for (line, code) in [
            (
                format!("{{\"v\":2,\"req\":\"shutdown\",\"nonce\":\"{NONCE}\"}}"),
                UNKNOWN_REQUEST,
            ),
            (
                format!("{{\"v\":2,\"req\":7,\"nonce\":\"{NONCE}\"}}"),
                MALFORMED,
            ),
            (
                format!("{{\"v\":2,\"req\":\"bootstrap\",\"nonce\":\"{NONCE}\",\"x\":1}}"),
                MALFORMED,
            ),
        ] {
            assert_eq!(
                parse_request(line.as_bytes()),
                Ok(ParsedRequest {
                    nonce: NONCE.to_owned(),
                    request: Err(code),
                }),
                "{line}"
            );
        }
    }

    fn line(reply: &Reply, proof: &str, nonce: &str) -> Vec<u8> {
        let encoded = without_newline(encode_response(proof, nonce, reply));
        assert!(encoded.len() < MAX_RESPONSE);
        encoded
    }

    #[test]
    fn a_response_verifies_only_with_the_proof_and_the_nonce() {
        let url = format!("http://127.0.0.1:43123/?bootstrap={CODE}");
        let reply = Reply::Url(url.clone());
        let encoded = line(&reply, PROOF, NONCE);
        assert!(
            !String::from_utf8_lossy(&encoded).contains(PROOF),
            "the proof never travels over the channel"
        );
        assert_eq!(parse_response(&encoded, PROOF, NONCE).ok(), Some(url));
        let other = "f".repeat(64);
        assert_eq!(
            parse_response(&line(&reply, &other, NONCE), PROOF, NONCE).err(),
            Some(InstanceError::Unauthenticated)
        );
        // A response recorded for another nonce is not accepted.
        assert_eq!(
            parse_response(&line(&reply, PROOF, &other), PROOF, NONCE).err(),
            Some(InstanceError::Unauthenticated)
        );
        assert_eq!(
            parse_response(&line(&Reply::Error(BUSY), PROOF, NONCE), PROOF, NONCE).err(),
            Some(InstanceError::Refused(BUSY.to_owned()))
        );
    }

    #[test]
    fn a_mac_covers_the_reply_itself() {
        let url = format!("http://127.0.0.1:43123/?bootstrap={CODE}");
        let mac = response_mac(PROOF, NONCE, "url", &url);
        let swapped = format!("http://127.0.0.1:43124/?bootstrap={CODE}");
        for response in [
            format!("{{\"v\":2,\"mac\":\"{mac}\",\"url\":\"{swapped}\"}}"),
            format!("{{\"v\":2,\"mac\":\"{mac}\",\"error\":\"instance.busy\"}}"),
            format!(
                "{{\"v\":2,\"mac\":\"{}\",\"url\":\"{url}\"}}",
                mac.to_uppercase()
            ),
        ] {
            assert_eq!(
                parse_response(response.as_bytes(), PROOF, NONCE).err(),
                Some(InstanceError::Unauthenticated),
                "{response}"
            );
        }
    }

    #[test]
    fn only_an_exact_loopback_bootstrap_url_is_accepted() {
        for url in [
            format!("https://127.0.0.1:43123/?bootstrap={CODE}"),
            format!("http://localhost:43123/?bootstrap={CODE}"),
            format!("http://127.0.0.1.evil.invalid:43123/?bootstrap={CODE}"),
            format!("http://127.0.0.1:0/?bootstrap={CODE}"),
            format!("http://127.0.0.1:70000/?bootstrap={CODE}"),
            format!("http://127.0.0.1:/?bootstrap={CODE}"),
            format!("http://127.0.0.1:43123/x?bootstrap={CODE}"),
            format!("http://127.0.0.1:43123/?bootstrap={CODE}&x=1"),
            format!("http://127.0.0.1:43123/?bootstrap={}", CODE.to_uppercase()),
            format!("http://127.0.0.1:43123/?bootstrap={}", &CODE[1..]),
            format!("http://127.0.0.1:43123/?bootstrap={CODE}\u{1b}[2J"),
        ] {
            assert!(!is_bootstrap_url(&url), "{url}");
            let reply = Reply::Url(url);
            assert_eq!(
                parse_response(&line(&reply, PROOF, NONCE), PROOF, NONCE).err(),
                Some(InstanceError::InvalidResponse)
            );
        }
    }

    #[test]
    fn malformed_responses_are_invalid() {
        let url = format!("http://127.0.0.1:43123/?bootstrap={CODE}");
        let mac = response_mac(PROOF, NONCE, "url", &url);
        let prose = response_mac(PROOF, NONCE, "error", "Some prose.");
        let empty = response_mac(PROOF, NONCE, "error", "");
        for response in [
            "not json".to_owned(),
            format!("{{\"v\":1,\"proof\":\"{PROOF}\",\"url\":\"{url}\"}}"),
            format!("{{\"v\":1,\"mac\":\"{mac}\",\"url\":\"{url}\"}}"),
            format!("{{\"v\":3,\"mac\":\"{mac}\",\"url\":\"{url}\"}}"),
            format!("{{\"v\":2,\"mac\":\"{mac}\"}}"),
            format!("{{\"v\":2,\"mac\":\"{mac}\",\"url\":\"{url}\",\"error\":\"instance.busy\"}}"),
            format!("{{\"v\":2,\"mac\":\"{mac}\",\"url\":\"{url}\",\"note\":\"x\"}}"),
            format!("{{\"v\":2,\"mac\":\"{prose}\",\"error\":\"Some prose.\"}}"),
            format!("{{\"v\":2,\"mac\":\"{empty}\",\"error\":\"\"}}"),
        ] {
            assert_eq!(
                parse_response(response.as_bytes(), PROOF, NONCE).err(),
                Some(InstanceError::InvalidResponse),
                "{response}"
            );
        }
    }

    #[test]
    fn endpoint_records_need_this_version_and_a_proof() {
        let record = EndpointRecord {
            v: VERSION,
            endpoint: "instance.sock".to_owned(),
            proof: PROOF.to_owned(),
        };
        let parsed = EndpointRecord::parse(&record.encode());
        assert_eq!(parsed.map(|record| record.proof), Some(PROOF.to_owned()));
        for bytes in [
            format!(r#"{{"v":1,"endpoint":"instance.sock","proof":"{PROOF}"}}"#).into_bytes(),
            format!(r#"{{"v":3,"endpoint":"instance.sock","proof":"{PROOF}"}}"#).into_bytes(),
            br#"{"v":2,"endpoint":"instance.sock","proof":"short"}"#.to_vec(),
            br#"{"v":2,"endpoint":"instance.sock"}"#.to_vec(),
            vec![b' '; MAX_ENDPOINT_FILE + 1],
        ] {
            assert!(EndpointRecord::parse(&bytes).is_none());
        }
    }
}
