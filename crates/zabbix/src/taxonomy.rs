// SPDX-License-Identifier: MIT OR Apache-2.0

//! Connection and authentication failure codes (P1-IN-01, P1-00F §3).
//!
//! Classification never relies on operating-system messages: those are
//! localized on Windows and unavailable for DNS on Linux. It uses the error
//! kind, the connection stage reported by the HTTP stack, rustls certificate
//! error variants and the English Zabbix API messages verified on 7.4.16.

use std::error::Error as StdError;
use std::io::ErrorKind;

/// Step of the connection check that a failure belongs to (P1-02D).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CheckStep {
    Endpoint,
    Api,
    Auth,
    Identity,
    Capabilities,
}

impl CheckStep {
    pub const ALL: [Self; 5] = [
        Self::Endpoint,
        Self::Api,
        Self::Auth,
        Self::Identity,
        Self::Capabilities,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Endpoint => "endpoint",
            Self::Api => "api",
            Self::Auth => "auth",
            Self::Identity => "identity",
            Self::Capabilities => "capabilities",
        }
    }
}

/// Stable wire code of a connection failure.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FailureCode {
    InvalidAddress,
    TrustMaterialInvalid,
    DnsUnresolvable,
    ConnectionRefused,
    HostUnreachableOrTimeout,
    TlsUntrusted,
    TlsCertificateTimeInvalid,
    TlsNameMismatch,
    ConnectionFailed,
    WrongBasePath,
    Redirect,
    NotJsonrpc,
    UnsupportedVersion,
    AuthFailed,
    TokenNotAuthorized,
    TokenExpired,
    MfaSessionTerminated,
    SessionExpired,
    SystemAccessDenied,
    ApiMethodDenied,
    ApiError,
    UnexpectedResponse,
}

impl FailureCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidAddress => "invalid_address",
            Self::TrustMaterialInvalid => "trust_material_invalid",
            Self::DnsUnresolvable => "dns_unresolvable",
            Self::ConnectionRefused => "connection_refused",
            Self::HostUnreachableOrTimeout => "host_unreachable_or_timeout",
            Self::TlsUntrusted => "tls_untrusted",
            Self::TlsCertificateTimeInvalid => "tls_certificate_time_invalid",
            Self::TlsNameMismatch => "tls_name_mismatch",
            Self::ConnectionFailed => "connection_failed",
            Self::WrongBasePath => "wrong_base_path",
            Self::Redirect => "redirect",
            Self::NotJsonrpc => "not_jsonrpc",
            Self::UnsupportedVersion => "unsupported_version",
            Self::AuthFailed => "auth_failed",
            Self::TokenNotAuthorized => "token_not_authorized",
            Self::TokenExpired => "token_expired",
            Self::MfaSessionTerminated => "mfa_session_terminated",
            Self::SessionExpired => "session_expired",
            Self::SystemAccessDenied => "system_access_denied",
            Self::ApiMethodDenied => "api_method_denied",
            Self::ApiError => "api_error",
            Self::UnexpectedResponse => "unexpected_response",
        }
    }

    /// Step where this failure is reported when it occurs while reaching the
    /// server. Failures of later calls are attributed by the caller.
    #[must_use]
    pub const fn default_step(self) -> CheckStep {
        match self {
            Self::InvalidAddress
            | Self::TrustMaterialInvalid
            | Self::DnsUnresolvable
            | Self::ConnectionRefused
            | Self::HostUnreachableOrTimeout
            | Self::TlsUntrusted
            | Self::TlsCertificateTimeInvalid
            | Self::TlsNameMismatch
            | Self::ConnectionFailed => CheckStep::Endpoint,
            Self::WrongBasePath
            | Self::Redirect
            | Self::NotJsonrpc
            | Self::UnsupportedVersion
            | Self::UnexpectedResponse => CheckStep::Api,
            Self::AuthFailed
            | Self::TokenNotAuthorized
            | Self::TokenExpired
            | Self::MfaSessionTerminated
            | Self::SessionExpired
            | Self::SystemAccessDenied => CheckStep::Auth,
            Self::ApiMethodDenied | Self::ApiError => CheckStep::Capabilities,
        }
    }

    /// Whether repeating the same request without changes may succeed.
    #[must_use]
    pub const fn retryable(self) -> bool {
        matches!(
            self,
            Self::DnsUnresolvable
                | Self::ConnectionRefused
                | Self::HostUnreachableOrTimeout
                | Self::ConnectionFailed
        )
    }
}

/// Safe parameter of a failure. Values never contain secrets, OS messages,
/// server response bodies or file paths.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailureParam {
    pub name: &'static str,
    pub value: String,
}

/// Classified failure: a code plus safe parameters, no prose.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Failure {
    pub code: FailureCode,
    pub params: Vec<FailureParam>,
}

impl Failure {
    #[must_use]
    pub fn new(code: FailureCode) -> Self {
        Self {
            code,
            params: Vec::new(),
        }
    }

    #[must_use]
    pub fn with(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.params.push(FailureParam {
            name,
            value: value.into(),
        });
        self
    }

    #[must_use]
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|param| param.name == name)
            .map(|param| param.value.as_str())
    }
}

/// One link of an error source chain, reduced to classification inputs.
#[derive(Clone, Debug, Default)]
pub(crate) struct ChainLink {
    display: String,
    debug: String,
    io_kind: Option<ErrorKind>,
    raw_os_error: Option<i32>,
}

pub(crate) fn chain_of(error: &(dyn StdError + 'static)) -> Vec<ChainLink> {
    let mut links = Vec::new();
    let mut current = Some(error);
    while let Some(error) = current {
        let io = error.downcast_ref::<std::io::Error>();
        links.push(ChainLink {
            display: error.to_string(),
            debug: format!("{error:?}"),
            io_kind: io.map(std::io::Error::kind),
            raw_os_error: io.and_then(std::io::Error::raw_os_error),
        });
        current = error.source();
    }
    links
}

/// Windows Sockets codes for an unknown host and a name without data.
const WSAHOST_NOT_FOUND: i32 = 11001;
const WSANO_DATA: i32 = 11004;

/// Classifies a transport failure from its source chain. `timed_out` is the
/// HTTP client's own timeout flag.
pub(crate) fn classify_transport(links: &[ChainLink], timed_out: bool) -> Failure {
    // TLS first: certificate errors arrive wrapped in I/O errors.
    for link in links {
        if let Some(code) = classify_certificate(&link.debug) {
            return Failure::new(code).with("stage", "tls");
        }
    }
    for link in links {
        if link.display == "dns error"
            || matches!(link.raw_os_error, Some(WSAHOST_NOT_FOUND | WSANO_DATA))
        {
            return Failure::new(FailureCode::DnsUnresolvable).with("stage", "dns");
        }
    }
    if timed_out {
        return Failure::new(FailureCode::HostUnreachableOrTimeout).with("stage", "connect");
    }
    for link in links {
        match link.io_kind {
            Some(ErrorKind::ConnectionRefused) => {
                return Failure::new(FailureCode::ConnectionRefused).with("stage", "connect");
            }
            Some(
                ErrorKind::TimedOut | ErrorKind::HostUnreachable | ErrorKind::NetworkUnreachable,
            ) => {
                return Failure::new(FailureCode::HostUnreachableOrTimeout)
                    .with("stage", "connect");
            }
            // Plain HTTP sent to a TLS port: the peer resets or answers with
            // bytes that are not HTTP (P1-IN-01).
            Some(ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted) => {
                return Failure::new(FailureCode::NotJsonrpc).with("stage", "http");
            }
            _ => {}
        }
    }
    for link in links {
        if link.debug.contains("IncompleteMessage") || link.debug.contains("Parse(Version") {
            return Failure::new(FailureCode::NotJsonrpc).with("stage", "http");
        }
    }
    Failure::new(FailureCode::ConnectionFailed).with("stage", "connect")
}

/// Maps rustls `CertificateError` variants (Debug names, not localized
/// text). Windows reports not-yet-valid certificates as `Expired`, so both
/// time cases share one code.
fn classify_certificate(debug: &str) -> Option<FailureCode> {
    let start = debug.find("InvalidCertificate(")?;
    let detail = &debug[start..];
    if detail.contains("NotValidForName") {
        Some(FailureCode::TlsNameMismatch)
    } else if detail.contains("Expired") || detail.contains("NotValidYet") {
        Some(FailureCode::TlsCertificateTimeInvalid)
    } else {
        Some(FailureCode::TlsUntrusted)
    }
}

/// Classification of a JSON-RPC error by the Zabbix 7.4 message texts.
pub(crate) fn classify_api(method: &str, code: i64, message: &str, data: Option<&str>) -> Failure {
    let lower = format!("{message} {}", data.unwrap_or_default()).to_ascii_lowercase();
    if lower.contains("api token expired") {
        Failure::new(FailureCode::TokenExpired)
    } else if lower.contains("not authorized") || lower.contains("not authorised") {
        Failure::new(FailureCode::TokenNotAuthorized)
    } else if lower.contains("session terminated") {
        Failure::new(FailureCode::SessionExpired)
    } else if lower.contains("no permissions for system access") {
        Failure::new(FailureCode::SystemAccessDenied)
    } else if lower.contains("no permissions to call") {
        Failure::new(FailureCode::ApiMethodDenied).with("method", method)
    } else if lower.contains("incorrect user name or password") {
        Failure::new(FailureCode::AuthFailed)
    } else {
        Failure::new(FailureCode::ApiError)
            .with("method", method)
            .with("api_code", code.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(display: &str, debug: &str, io_kind: Option<ErrorKind>, os: Option<i32>) -> ChainLink {
        ChainLink {
            display: display.to_owned(),
            debug: debug.to_owned(),
            io_kind,
            raw_os_error: os,
        }
    }

    #[test]
    fn certificate_variants_from_both_verifiers_are_classified() {
        let cases = [
            (
                "InvalidCertificate(UnknownIssuer)",
                FailureCode::TlsUntrusted,
            ),
            (
                "InvalidCertificate(NotValidForName)",
                FailureCode::TlsNameMismatch,
            ),
            (
                "InvalidCertificate(NotValidForNameContext { expected: .. })",
                FailureCode::TlsNameMismatch,
            ),
            (
                "InvalidCertificate(Expired)",
                FailureCode::TlsCertificateTimeInvalid,
            ),
            (
                "InvalidCertificate(ExpiredContext { time: .. })",
                FailureCode::TlsCertificateTimeInvalid,
            ),
            (
                "InvalidCertificate(NotValidYetContext { time: .. })",
                FailureCode::TlsCertificateTimeInvalid,
            ),
            (
                "InvalidCertificate(BadSignature)",
                FailureCode::TlsUntrusted,
            ),
        ];
        for (debug, expected) in cases {
            let links = [
                link(
                    "client error (Connect)",
                    "Error { kind: Connect }",
                    None,
                    None,
                ),
                link(
                    "invalid peer certificate",
                    &format!("Custom {{ kind: InvalidData, error: {debug} }}"),
                    Some(ErrorKind::InvalidData),
                    None,
                ),
            ];
            let failure = classify_transport(&links, false);
            assert_eq!(failure.code, expected, "{debug}");
            assert_eq!(failure.param("stage"), Some("tls"));
        }
    }

    #[test]
    fn dns_is_detected_by_stage_on_linux_and_os_code_on_windows() {
        let linux = [link("dns error", "ConnectError(\"dns error\")", None, None)];
        assert_eq!(
            classify_transport(&linux, false).code,
            FailureCode::DnsUnresolvable
        );
        let windows = [link(
            "localized text",
            "Os { code: 11001 }",
            Some(ErrorKind::Other),
            Some(11001),
        )];
        assert_eq!(
            classify_transport(&windows, false).code,
            FailureCode::DnsUnresolvable
        );
    }

    #[test]
    fn socket_kinds_map_to_endpoint_codes() {
        let refused = [link(
            "x",
            "x",
            Some(ErrorKind::ConnectionRefused),
            Some(111),
        )];
        assert_eq!(
            classify_transport(&refused, false).code,
            FailureCode::ConnectionRefused
        );
        for kind in [
            ErrorKind::TimedOut,
            ErrorKind::HostUnreachable,
            ErrorKind::NetworkUnreachable,
        ] {
            let links = [link("x", "x", Some(kind), None)];
            assert_eq!(
                classify_transport(&links, false).code,
                FailureCode::HostUnreachableOrTimeout
            );
        }
        assert_eq!(
            classify_transport(&[], true).code,
            FailureCode::HostUnreachableOrTimeout
        );
        let reset = [link("x", "x", Some(ErrorKind::ConnectionReset), Some(104))];
        assert_eq!(
            classify_transport(&reset, false).code,
            FailureCode::NotJsonrpc
        );
        let incomplete = [link("x", "hyper::Error(IncompleteMessage)", None, None)];
        assert_eq!(
            classify_transport(&incomplete, false).code,
            FailureCode::NotJsonrpc
        );
        assert_eq!(
            classify_transport(&[link("x", "x", None, None)], false).code,
            FailureCode::ConnectionFailed
        );
    }

    #[test]
    fn zabbix_messages_from_the_laboratory_are_classified() {
        let cases = [
            (
                "user.login",
                -32500,
                "Application error.",
                Some("Incorrect user name or password or account is temporarily blocked."),
                FailureCode::AuthFailed,
            ),
            (
                "user.login",
                -32602,
                "Invalid params.",
                Some("No permissions for system access."),
                FailureCode::SystemAccessDenied,
            ),
            (
                "user.checkAuthentication",
                -32602,
                "Invalid params.",
                Some("Not authorized."),
                FailureCode::TokenNotAuthorized,
            ),
            (
                "user.get",
                -32500,
                "Application error.",
                Some("API token expired."),
                FailureCode::TokenExpired,
            ),
            (
                "user.get",
                -32602,
                "Invalid params.",
                Some("Session terminated, re-login, please."),
                FailureCode::SessionExpired,
            ),
            (
                "template.get",
                -32500,
                "Application error.",
                Some("No permissions to call \"template.get\"."),
                FailureCode::ApiMethodDenied,
            ),
            (
                "user.get",
                -32602,
                "Invalid params.",
                Some("Invalid parameter \"/\": unexpected parameter \"x\"."),
                FailureCode::ApiError,
            ),
        ];
        for (method, code, message, data, expected) in cases {
            assert_eq!(classify_api(method, code, message, data).code, expected);
        }
        let denied = classify_api(
            "template.get",
            -32500,
            "Application error.",
            Some("No permissions to call \"template.get\"."),
        );
        assert_eq!(denied.param("method"), Some("template.get"));
    }
}
