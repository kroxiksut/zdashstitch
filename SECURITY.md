# Security policy (pre-implementation)

ZDashstitch is not yet a released executable. Security-sensitive reports about the schemas, reference parsers or future implementation should be disclosed privately through the repository's security advisory/private vulnerability reporting feature **if enabled**; otherwise open an issue without exploit details or secrets and request a private contact channel.

Never attach private Zabbix API tokens, credentials, internal hostnames/IPs, unsanitized dashboards or enterprise exports to public issues. Portable YAML/ZIP is **not automatically anonymized**.

Core design rules: untrusted imports are data-only; no arbitrary code in MVP packages; explicit human confirmation before write; least-privilege API access; TLS verification, origin/host/session validation for local HTTP; per-version/per-widget confirmed capability gates; unknown outcome requires read-back rather than blind replay. P0 lab testing must not modify production dashboards.

The contents of this file describe intended security requirements, **not a claim that these controls have been implemented**.
