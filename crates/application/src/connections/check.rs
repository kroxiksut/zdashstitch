// SPDX-License-Identifier: MIT OR Apache-2.0

//! Step-by-step connection check as data (P2-01F, P1-02D): endpoint/TLS →
//! API → sign-in → server identity → permissions and capabilities. A step
//! starts only after the previous one passed; later steps are reported as
//! not checked. The report holds codes and safe parameters only.

use zdashstitch_zabbix::{
    AuthMethod, Authentication, CheckStep, Failure, FailureCode, FailureParam, ReadAccess,
    ReadMethod, ReadOnlyGateway, ServerIdentity, TrustAnchors, ZabbixClient, ZabbixEndpoint,
};

use super::profile::StoredIdentity;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StepStatus {
    Passed,
    /// Passed with information the user must see (e.g. denied read methods,
    /// a server upgrade, a first-seen identity).
    Notice,
    Failed,
    NotChecked,
}

impl StepStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Notice => "notice",
            Self::Failed => "failed",
            Self::NotChecked => "not_checked",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StepReport {
    pub step: CheckStep,
    pub status: StepStatus,
    /// Failure or notice code; for `not_checked` the reason.
    pub code: Option<&'static str>,
    pub params: Vec<FailureParam>,
}

impl StepReport {
    fn new(step: CheckStep, status: StepStatus, code: Option<&'static str>) -> Self {
        Self {
            step,
            status,
            code,
            params: Vec::new(),
        }
    }

    fn with(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.params.push(FailureParam {
            name,
            value: value.into(),
        });
        self
    }
}

/// Identity of the server compared with the profile (step 4).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityMatch {
    /// No identity stored yet; it is recorded on connect.
    FirstSeen,
    Matches,
    /// Same server and account, other version: plans become invalid.
    VersionChanged,
    /// Same address, different server or account: explicit confirmation.
    Changed,
}

impl IdentityMatch {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FirstSeen => "identity_first_seen",
            Self::Matches => "identity_matches",
            Self::VersionChanged => "server_version_changed",
            Self::Changed => "server_identity_changed",
        }
    }
}

/// Result of a connection check. Contains no secret, server response body,
/// OS message or file path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectionCheckReport {
    pub steps: Vec<StepReport>,
    pub auth_method: AuthMethod,
    pub server_version: Option<String>,
    pub identity: Option<IdentityMatch>,
    pub read_access: Vec<(ReadMethod, ReadAccess)>,
    /// Unix seconds.
    pub checked_at: u64,
    /// The sign-in passed with the public Zabbix default account
    /// (`Admin` / `zabbix`) and the profile has not hidden the warning
    /// (O-037). Set by the service; the credentials are compared in memory
    /// and never logged.
    pub default_password_warning: bool,
}

impl ConnectionCheckReport {
    /// First failure code, `None` when every step passed.
    #[must_use]
    pub fn result_code(&self) -> Option<&'static str> {
        self.steps
            .iter()
            .find(|step| step.status == StepStatus::Failed)
            .and_then(|step| step.code)
    }

    #[must_use]
    pub fn passed(&self) -> bool {
        self.result_code().is_none()
    }

    #[must_use]
    pub fn failed_step(&self) -> Option<CheckStep> {
        self.steps
            .iter()
            .find(|step| step.status == StepStatus::Failed)
            .map(|step| step.step)
    }
}

pub(crate) struct CheckOutcome {
    pub report: ConnectionCheckReport,
    /// Present when sign-in succeeded, even if identity or capabilities
    /// failed, so the caller can close the session.
    pub gateway: Option<ReadOnlyGateway>,
    pub observed: Option<ServerIdentity>,
}

fn failed(step: CheckStep, failure: &Failure) -> StepReport {
    StepReport {
        step,
        status: StepStatus::Failed,
        code: Some(failure.code.as_str()),
        params: failure.params.clone(),
    }
}

fn finish(
    mut steps: Vec<StepReport>,
    auth_method: AuthMethod,
    server_version: Option<String>,
    checked_at: u64,
) -> ConnectionCheckReport {
    for step in CheckStep::ALL.into_iter().skip(steps.len()) {
        steps.push(StepReport::new(
            step,
            StepStatus::NotChecked,
            Some("previous_step_failed"),
        ));
    }
    ConnectionCheckReport {
        steps,
        auth_method,
        server_version,
        identity: None,
        read_access: Vec::new(),
        checked_at,
        default_password_warning: false,
    }
}

pub(crate) fn compare_identity(
    stored: Option<&StoredIdentity>,
    observed: &ServerIdentity,
) -> IdentityMatch {
    match stored {
        None => IdentityMatch::FirstSeen,
        Some(stored) if stored.server_fingerprint != observed.fingerprint() => {
            IdentityMatch::Changed
        }
        Some(stored) if stored.server_version != observed.version().to_string() => {
            IdentityMatch::VersionChanged
        }
        Some(_) => IdentityMatch::Matches,
    }
}

pub(crate) async fn run_check(
    endpoint: ZabbixEndpoint,
    trust: &TrustAnchors,
    authentication: Authentication,
    stored_identity: Option<&StoredIdentity>,
    accept_changed_identity: bool,
    checked_at: u64,
) -> CheckOutcome {
    let auth_method = authentication.method();
    let mut steps = Vec::with_capacity(CheckStep::ALL.len());

    // Steps 1–2: one unauthenticated apiinfo.version call; the failure code
    // tells whether the endpoint/TLS or the API itself failed.
    let client = match ZabbixClient::with_trust(endpoint, trust) {
        Ok(client) => client,
        Err(error) => {
            steps.push(failed(CheckStep::Endpoint, &error.failure()));
            return CheckOutcome {
                report: finish(steps, auth_method, None, checked_at),
                gateway: None,
                observed: None,
            };
        }
    };
    let version = match client.detect_version().await {
        Ok(version) => version,
        Err(error) => {
            let failure = error.failure();
            if error.check_step() == CheckStep::Endpoint {
                steps.push(failed(CheckStep::Endpoint, &failure));
            } else {
                steps.push(StepReport::new(
                    CheckStep::Endpoint,
                    StepStatus::Passed,
                    None,
                ));
                steps.push(failed(CheckStep::Api, &failure));
            }
            return CheckOutcome {
                report: finish(steps, auth_method, None, checked_at),
                gateway: None,
                observed: None,
            };
        }
    };
    steps.push(StepReport::new(
        CheckStep::Endpoint,
        StepStatus::Passed,
        None,
    ));
    let server_version = Some(version.to_string());
    if let Err(error) = zdashstitch_zabbix::AdapterRegistry::resolve(version.clone()) {
        steps.push(failed(CheckStep::Api, &error.failure()));
        return CheckOutcome {
            report: finish(steps, auth_method, server_version, checked_at),
            gateway: None,
            observed: None,
        };
    }
    steps.push(
        StepReport::new(CheckStep::Api, StepStatus::Passed, None)
            .with("version", version.to_string()),
    );

    // Step 3.
    let gateway = match client.sign_in(version, authentication).await {
        Ok(gateway) => gateway,
        Err(error) => {
            let failure = error.failure();
            let mut report_step = failed(CheckStep::Auth, &failure);
            if error.check_step() == CheckStep::Endpoint {
                report_step.params.push(FailureParam {
                    name: "during",
                    value: "sign_in".to_owned(),
                });
            }
            steps.push(report_step);
            return CheckOutcome {
                report: finish(steps, auth_method, server_version, checked_at),
                gateway: None,
                observed: None,
            };
        }
    };
    steps.push(
        StepReport::new(CheckStep::Auth, StepStatus::Passed, None)
            .with("auth_method", auth_method.as_str()),
    );

    // Step 4.
    let observed = gateway.identity().clone();
    let identity = compare_identity(stored_identity, &observed);
    let identity_step = match identity {
        IdentityMatch::Matches => StepReport::new(CheckStep::Identity, StepStatus::Passed, None),
        IdentityMatch::FirstSeen => StepReport::new(
            CheckStep::Identity,
            StepStatus::Notice,
            Some(identity.as_str()),
        ),
        IdentityMatch::VersionChanged => StepReport::new(
            CheckStep::Identity,
            StepStatus::Notice,
            Some(identity.as_str()),
        )
        .with(
            "previous_version",
            stored_identity
                .map(|stored| stored.server_version.clone())
                .unwrap_or_default(),
        )
        .with("version", observed.version().to_string()),
        IdentityMatch::Changed if accept_changed_identity => StepReport::new(
            CheckStep::Identity,
            StepStatus::Notice,
            Some(identity.as_str()),
        )
        .with("accepted", "true"),
        IdentityMatch::Changed => StepReport::new(
            CheckStep::Identity,
            StepStatus::Failed,
            Some(identity.as_str()),
        ),
    };
    steps.push(identity_step);
    if identity == IdentityMatch::Changed && !accept_changed_identity {
        let mut report = finish(steps, auth_method, server_version, checked_at);
        report.identity = Some(identity);
        return CheckOutcome {
            report,
            gateway: Some(gateway),
            observed: Some(observed),
        };
    }

    // Step 5.
    let (capability_step, read_access) = match gateway.probe_read_access().await {
        Ok(access) => {
            let denied: Vec<&str> = access
                .iter()
                .filter(|(_, state)| *state == ReadAccess::Denied)
                .map(|(method, _)| method.api_method())
                .collect();
            let step = if denied.is_empty() {
                StepReport::new(CheckStep::Capabilities, StepStatus::Passed, None)
            } else {
                StepReport::new(
                    CheckStep::Capabilities,
                    StepStatus::Notice,
                    Some(FailureCode::ApiMethodDenied.as_str()),
                )
                .with("methods", denied.join(","))
            };
            (step, access)
        }
        Err(error) => (
            failed(CheckStep::Capabilities, &error.failure()),
            Vec::new(),
        ),
    };
    steps.push(capability_step);
    let mut report = finish(steps, auth_method, server_version, checked_at);
    report.identity = Some(identity);
    report.read_access = read_access;
    CheckOutcome {
        report,
        gateway: Some(gateway),
        observed: Some(observed),
    }
}
