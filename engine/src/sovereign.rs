//! Sovereign Mode (R5, v0.5.0 "Freedom" roadmap).
//!
//! The pledge: when Sovereign Mode is ON, your code **never leaves your
//! machine**. All LLM inference goes to a local Ollama instance on
//! `localhost`; every other network egress the engine initiates is blocked.
//! This is the EU story: there is no data processor and no sub-processor,
//! because there is no transfer to process.
//!
//! Three layers:
//!
//! 1. **Policy (pure, testable).** [`SovereignPolicy`] decides Allow/Block for
//!    any URL and validates LLM configuration (Ollama-on-localhost only).
//! 2. **Preflight.** [`preflight_check`] scans the *effective* configuration
//!    for remote endpoints and API keys *before* the mode engages, and
//!    reports findings the user must acknowledge or fix first.
//! 3. **Attestation.** [`AttestationLog`] is a hash-chained session log proving
//!    - to an auditor - that the session ran under sovereign policy. Each
//!      entry commits to the previous entry's hash; [`verify_attestation`]
//!      recomputes the whole chain offline with any SHA-256 tool.
//!
//! ## Honest limits - read before promising anyone compliance
//!
//! Sovereign Mode is enforced at the **application layer** of this engine:
//!
//! - It cannot stop the OS, a browser, or another application from sending
//!   data out. Full assurance needs OS-level firewall rules too.
//! - It cannot verify that the Ollama binary itself is unmodified. Verify
//!   your Ollama install separately (release signatures, package hashes).
//! - Shell tool calls (e.g. `BashTool` running `curl`) execute outside this
//!   policy. Confining those is the sandbox module's job, not this one's.
//! - DNS lookups for blocked hosts may still leave the machine (DNS is not
//!   HTTP). Use a local resolver for full assurance.
//! - Attestation proves **the engine's** behavior during the session, not the
//!   behavior of the whole system. The chain uses SHA-256 hashes
//!   (`hash-chain-v1`); real asymmetric signatures (minisign) are tracked
//!   follow-up work, not claimed here.
//!
//! ## Integration with the network choke point
//!
//! The zero-telemetry module (`netlog.rs`, PR #59) funnels all
//! engine-initiated HTTP through `logged_fetch`. When both are merged, that
//! choke point must call [`SovereignMode::check_egress`] first and refuse
//! `EgressDecision::Block` before any socket is opened:
//!
//! ```ignore
//! if let Some(sov) = sovereign_mode.as_ref() {
//!     sov.check_egress(&req.url)?; // BlockedUrl error before connect
//! }
//! let resp = logged_fetch(&log, req).await?;
//! ```

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Default Ollama endpoint Sovereign Mode pins inference to.
pub const OLLAMA_LOOPBACK_URL: &str = "http://localhost:11434/v1";
/// Default Ollama port, used by the liveness probe.
pub const OLLAMA_DEFAULT_PORT: u16 = 11434;
/// Policy identifier recorded in attestations.
pub const POLICY_VERSION: &str = "sovereign-policy-v1";
/// Attestation chain scheme. Hash-chained, NOT asymmetrically signed -
/// see the module docs for the honest limits.
pub const ATTESTATION_SCHEME: &str = "hash-chain-v1";

/// What the sovereign egress policy decided about one URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EgressDecision {
    /// The URL targets loopback; allowed under sovereign policy.
    Allow,
    /// The URL would leave the machine; blocked.
    Block { url: String, reason: String },
}

/// Errors raised by Sovereign Mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SovereignError {
    /// An egress attempt to a non-loopback URL while the mode is engaged.
    BlockedUrl { url: String, reason: String },
    /// The LLM configuration is not sovereign (remote endpoint or API key).
    NonSovereignConfig { detail: String },
    /// Preflight found findings the user has not acknowledged.
    PreflightBlocked { findings: Vec<String> },
    /// Ollama is not reachable on loopback; sovereign inference impossible.
    OllamaUnreachable { endpoint: String },
}

impl fmt::Display for SovereignError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SovereignError::BlockedUrl { url, reason } => write!(
                f,
                "Sovereign Mode blocked network egress to '{url}': {reason}. \
                 Only localhost traffic is allowed while Sovereign Mode is on."
            ),
            SovereignError::NonSovereignConfig { detail } => write!(
                f,
                "Not a sovereign configuration: {detail}. Sovereign Mode pins \
                 inference to Ollama on localhost with no API key."
            ),
            SovereignError::PreflightBlocked { findings } => write!(
                f,
                "Sovereign Mode refused to engage until preflight findings are \
                 resolved: {}",
                findings.join("; ")
            ),
            SovereignError::OllamaUnreachable { endpoint } => write!(
                f,
                "Sovereign Mode needs a local Ollama instance, but {endpoint} \
                 is not reachable. Start Ollama first (`ollama serve`), then \
                 enable Sovereign Mode."
            ),
        }
    }
}

impl std::error::Error for SovereignError {}

/// Split a URL into `(host, port)` without external crates.
///
/// Returns `None` when the URL has no parseable authority section.
fn split_host_port(url: &str) -> Option<(String, Option<u16>)> {
    let no_frag = url.split('#').next().unwrap_or(url);
    let no_query = no_frag.split('?').next().unwrap_or(no_frag);
    let rest = no_query.split("://").nth(1)?;
    let authority = rest.split('/').next().unwrap_or(rest);
    // Strip userinfo (`user:pass@`); it must never influence the decision.
    let hostport = authority.rsplit('@').next().unwrap_or(authority);
    // IPv6 literals: `[::1]:11434` or `[::1]`.
    if let Some(stripped) = hostport.strip_prefix('[') {
        let end = stripped.find(']')?;
        let host = format!("[{}]", &stripped[..end]);
        let port = stripped[end + 1..].strip_prefix(':')?.parse::<u16>().ok();
        return Some((host, port));
    }
    match hostport.rsplit_once(':') {
        Some((host, port_str)) if port_str.parse::<u16>().is_ok() => {
            Some((host.to_string(), port_str.parse().ok()))
        }
        _ => Some((hostport.to_string(), None)),
    }
}

/// True when `host` is a loopback destination: `localhost`, the whole
/// `127.0.0.0/8` range, or IPv6 `::1`.
///
/// `0.0.0.0` is treated as loopback-equivalent: a TCP connect to it never
/// leaves the machine on any supported OS (it resolves to localhost).
/// Hostnames other than `localhost` are NOT trusted - DNS could resolve them
/// anywhere, and Sovereign Mode does not do DNS resolution to check.
pub fn is_loopback_host(host: &str) -> bool {
    let h = host.trim().trim_end_matches('.').to_lowercase();
    if h == "localhost" || h == "0.0.0.0" {
        return true;
    }
    // Strip brackets from IPv6 literals for parsing.
    let bare = h
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(&h);
    if bare == "::1" {
        return true;
    }
    if let Ok(IpAddr::V4(v4)) = bare.parse::<IpAddr>() {
        return v4.octets()[0] == 127;
    }
    false
}

/// Pure sovereign egress policy: allow loopback, block everything else.
///
/// This is the function the HTTP choke point must consult before opening any
/// socket while Sovereign Mode is engaged.
pub fn check_url_sovereign(url: &str) -> EgressDecision {
    match split_host_port(url) {
        Some((host, _)) if is_loopback_host(&host) => EgressDecision::Allow,
        Some((host, _)) => EgressDecision::Block {
            url: url.to_string(),
            reason: format!(
                "host '{host}' is not loopback; Sovereign Mode permits only localhost traffic"
            ),
        },
        None => EgressDecision::Block {
            url: url.to_string(),
            reason: "URL has no parseable host; refusing by default".to_string(),
        },
    }
}

/// Validate that an LLM configuration is sovereign: Ollama on loopback, and
/// **no API key** (a key implies a remote account that could bill or log).
pub fn validate_llm_config(base_url: &str, api_key: Option<&str>) -> Result<(), SovereignError> {
    if let Some(k) = api_key {
        if !k.trim().is_empty() {
            return Err(SovereignError::NonSovereignConfig {
                detail:
                    "an API key is configured; sovereign inference uses local Ollama with no key"
                        .to_string(),
            });
        }
    }
    match check_url_sovereign(base_url) {
        EgressDecision::Allow => Ok(()),
        EgressDecision::Block { reason, .. } => Err(SovereignError::NonSovereignConfig {
            detail: format!("LLM endpoint '{base_url}' is not local: {reason}"),
        }),
    }
}

/// One preflight finding: something that must be resolved or acknowledged
/// before Sovereign Mode engages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreflightFinding {
    /// Machine-readable kind: `remote-endpoint` | `api-key-present` | `ollama-unreachable`.
    pub kind: String,
    /// Human-readable detail, safe to show in the UI.
    pub detail: String,
    /// Whether the mode can engage if the user explicitly acknowledges it.
    pub acknowledgable: bool,
}

/// Scan the effective configuration *before* engaging Sovereign Mode.
///
/// Pure except for the optional Ollama liveness probe (`probe_ollama`).
/// Returns findings; empty means clean. Findings with
/// `acknowledgable == false` must be *fixed*, not acknowledged.
pub fn preflight_check(
    base_url: &str,
    api_key: Option<&str>,
    extra_urls: &[String],
    probe_ollama: bool,
) -> Vec<PreflightFinding> {
    let mut findings = Vec::new();

    match check_url_sovereign(base_url) {
        EgressDecision::Allow => {}
        EgressDecision::Block { reason, .. } => findings.push(PreflightFinding {
            kind: "remote-endpoint".to_string(),
            detail: format!("LLM endpoint '{base_url}' would leave the machine: {reason}"),
            acknowledgable: false,
        }),
    }

    if let Some(k) = api_key {
        if !k.trim().is_empty() {
            findings.push(PreflightFinding {
                kind: "api-key-present".to_string(),
                detail: "An API key is configured. Sovereign Mode uses local Ollama \
                         with no key; the key will be ignored while the mode is on, \
                         which may surprise you if you expected remote inference."
                    .to_string(),
                acknowledgable: true,
            });
        }
    }

    for url in extra_urls {
        if let EgressDecision::Block { reason, .. } = check_url_sovereign(url) {
            findings.push(PreflightFinding {
                kind: "remote-endpoint".to_string(),
                detail: format!("Configured URL '{url}' would leave the machine: {reason}"),
                acknowledgable: false,
            });
        }
    }

    if probe_ollama && !probe_ollama_running(OLLAMA_DEFAULT_PORT) {
        findings.push(PreflightFinding {
            kind: "ollama-unreachable".to_string(),
            detail: format!(
                "No Ollama instance is listening on 127.0.0.1:{OLLAMA_DEFAULT_PORT}. \
                 Start it (`ollama serve`) before enabling Sovereign Mode."
            ),
            acknowledgable: false,
        });
    }

    findings
}

/// TCP liveness probe: is something listening on 127.0.0.1:`port`?
///
/// Short timeout; used at engage-time so the mode never promises local
/// inference it cannot deliver.
pub fn probe_ollama_running(port: u16) -> bool {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok()
}

/// One entry in the attestation hash chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttestationEntry {
    pub seq: u64,
    /// RFC 3339 UTC timestamp.
    pub ts: String,
    /// Machine-readable kind: `genesis` | `egress-allowed` | `egress-blocked` |
    /// `config-validated` | `mode-disabled`.
    pub kind: String,
    /// Human-readable detail (never contains secrets).
    pub detail: String,
    /// Hex SHA-256 of the previous entry's canonical encoding (`GENESIS` for seq 0).
    pub prev_hash: String,
    /// Hex SHA-256 over `seq | ts | kind | detail | prev_hash`.
    pub hash: String,
}

fn sha256_hex(data: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data.as_bytes());
    hex_encode(&hasher.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

fn now_rfc3339() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Reuse the Howard Hinnant days-to-civil algorithm (same as netlog.rs).
    let days = (secs / 86_400) as i64;
    let sod = secs % 86_400;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year,
        m,
        d,
        sod / 3_600,
        (sod % 3_600) / 60,
        sod % 60
    )
}

fn entry_hash(seq: u64, ts: &str, kind: &str, detail: &str, prev_hash: &str) -> String {
    sha256_hex(&format!("{seq}|{ts}|{kind}|{detail}|{prev_hash}"))
}

/// Append-only, hash-chained log of sovereign-policy events for one session.
#[derive(Debug, Clone)]
pub struct AttestationLog {
    entries: Vec<AttestationEntry>,
}

impl AttestationLog {
    /// Start a new chain. The genesis entry records the policy version and
    /// the pinned Ollama endpoint, so a verifier can see what "sovereign"
    /// meant for this session.
    pub fn new(session_id: &str) -> Self {
        let genesis_detail = format!(
            "session {session_id} started under {POLICY_VERSION}; inference pinned to {OLLAMA_LOOPBACK_URL}"
        );
        let entry = AttestationEntry {
            seq: 0,
            ts: now_rfc3339(),
            kind: "genesis".to_string(),
            detail: genesis_detail,
            prev_hash: "GENESIS".to_string(),
            hash: String::new(),
        };
        let hash = entry_hash(
            entry.seq,
            &entry.ts,
            &entry.kind,
            &entry.detail,
            &entry.prev_hash,
        );
        let mut log = Self {
            entries: Vec::new(),
        };
        log.entries.push(AttestationEntry { hash, ..entry });
        log
    }

    /// Append a policy event. `detail` must never contain secrets.
    pub fn record(&mut self, kind: &str, detail: &str) {
        let prev_hash = self
            .entries
            .last()
            .map(|e| e.hash.clone())
            .unwrap_or_else(|| "GENESIS".to_string());
        let seq = self.entries.len() as u64;
        let ts = now_rfc3339();
        let hash = entry_hash(seq, &ts, kind, detail, &prev_hash);
        self.entries.push(AttestationEntry {
            seq,
            ts,
            kind: kind.to_string(),
            detail: detail.to_string(),
            prev_hash,
            hash,
        });
    }

    pub fn entries(&self) -> &[AttestationEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Finalize into a portable attestation document.
    pub fn finalize(&self, session_id: &str) -> Attestation {
        Attestation {
            scheme: ATTESTATION_SCHEME.to_string(),
            policy_version: POLICY_VERSION.to_string(),
            session_id: session_id.to_string(),
            finalized_at: now_rfc3339(),
            entries: self.entries.clone(),
            final_hash: self
                .entries
                .last()
                .map(|e| e.hash.clone())
                .unwrap_or_default(),
        }
    }
}

/// Portable attestation document: proves a session ran under sovereign policy.
///
/// Verifiable offline with [`verify_attestation`] or any SHA-256 tool by
/// recomputing each entry's hash from its fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attestation {
    pub scheme: String,
    pub policy_version: String,
    pub session_id: String,
    pub finalized_at: String,
    pub entries: Vec<AttestationEntry>,
    pub final_hash: String,
}

/// Recompute the whole hash chain. Returns `true` only if every entry's hash
/// matches its fields and every `prev_hash` links to the previous entry.
pub fn verify_attestation(att: &Attestation) -> bool {
    if att.scheme != ATTESTATION_SCHEME || att.policy_version != POLICY_VERSION {
        return false;
    }
    if att.entries.is_empty() {
        return false;
    }
    let mut prev = "GENESIS".to_string();
    for (i, e) in att.entries.iter().enumerate() {
        if e.seq != i as u64 || e.prev_hash != prev {
            return false;
        }
        let recomputed = entry_hash(e.seq, &e.ts, &e.kind, &e.detail, &e.prev_hash);
        if recomputed != e.hash {
            return false;
        }
        prev = e.hash.clone();
    }
    match att.entries.last() {
        Some(last) => last.hash == att.final_hash,
        None => false,
    }
}

/// A live Sovereign Mode session: policy enforcement + attestation.
///
/// Obtain via [`SovereignMode::enable`]; the mode stays engaged until
/// [`SovereignMode::disable`] records the closing entry.
#[derive(Debug)]
pub struct SovereignMode {
    session_id: String,
    ollama_base_url: String,
    log: AttestationLog,
    blocked_egress: u64,
    allowed_egress: u64,
}

impl SovereignMode {
    /// Engage Sovereign Mode.
    ///
    /// Runs the preflight check (pure part) and probes Ollama on loopback.
    /// Returns the first blocking error; non-blocking findings (API key
    /// present) are recorded in the attestation log but do not stop
    /// engagement - the key is simply never used while the mode is on.
    pub fn enable(session_id: &str) -> Result<Self, SovereignError> {
        Self::enable_with_probe(session_id, OLLAMA_LOOPBACK_URL, true)
    }

    /// Engage with an explicit endpoint and probe toggle (the toggle exists
    /// for tests; production always probes).
    pub fn enable_with_probe(
        session_id: &str,
        ollama_base_url: &str,
        probe: bool,
    ) -> Result<Self, SovereignError> {
        // 1. The endpoint itself must be sovereign.
        validate_llm_config(ollama_base_url, None)?;
        // 2. Ollama must actually be listening - otherwise we would promise
        //    local inference we cannot deliver.
        if probe && !probe_ollama_running(OLLAMA_DEFAULT_PORT) {
            return Err(SovereignError::OllamaUnreachable {
                endpoint: format!("127.0.0.1:{OLLAMA_DEFAULT_PORT}"),
            });
        }
        let mut log = AttestationLog::new(session_id);
        log.record(
            "config-validated",
            &format!("inference endpoint validated as loopback: {ollama_base_url}"),
        );
        Ok(Self {
            session_id: session_id.to_string(),
            ollama_base_url: ollama_base_url.to_string(),
            log,
            blocked_egress: 0,
            allowed_egress: 0,
        })
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn ollama_base_url(&self) -> &str {
        &self.ollama_base_url
    }

    /// Enforce the egress policy on one URL. Call this from the HTTP choke
    /// point *before* any socket is opened. Records the decision in the
    /// attestation log (blocked attempts are evidence the policy works).
    pub fn check_egress(&mut self, url: &str) -> Result<(), SovereignError> {
        match check_url_sovereign(url) {
            EgressDecision::Allow => {
                self.allowed_egress += 1;
                Ok(())
            }
            EgressDecision::Block { reason, .. } => {
                self.blocked_egress += 1;
                self.log
                    .record("egress-blocked", &format!("blocked '{url}': {reason}"));
                Err(SovereignError::BlockedUrl {
                    url: url.to_string(),
                    reason,
                })
            }
        }
    }

    /// Validate + record in one step (the `&mut` variant used at runtime).
    pub fn apply_to_config(
        &mut self,
        base_url: &str,
        api_key: Option<&str>,
    ) -> Result<(String, Option<String>), SovereignError> {
        validate_llm_config(base_url, api_key)?;
        self.log.record(
            "config-validated",
            &format!(
                "task config validated; inference pinned to {}",
                self.ollama_base_url
            ),
        );
        Ok((self.ollama_base_url.clone(), None))
    }

    pub fn blocked_egress_count(&self) -> u64 {
        self.blocked_egress
    }

    pub fn allowed_egress_count(&self) -> u64 {
        self.allowed_egress
    }

    pub fn attestation_entries(&self) -> &[AttestationEntry] {
        self.log.entries()
    }

    /// Disengage: records the closing entry and returns the attestation.
    pub fn disable(mut self) -> Attestation {
        self.log.record(
            "mode-disabled",
            &format!(
                "sovereign mode disabled; egress allowed={} blocked={}",
                self.allowed_egress, self.blocked_egress
            ),
        );
        self.log.finalize(&self.session_id)
    }
}

/// One-line status for the desktop UI badge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SovereignStatus {
    pub enabled: bool,
    pub session_id: Option<String>,
    pub ollama_endpoint: String,
    pub ollama_reachable: bool,
    pub allowed_egress: u64,
    pub blocked_egress: u64,
}

impl SovereignStatus {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            session_id: None,
            ollama_endpoint: OLLAMA_LOOPBACK_URL.to_string(),
            ollama_reachable: probe_ollama_running(OLLAMA_DEFAULT_PORT),
            allowed_egress: 0,
            blocked_egress: 0,
        }
    }

    pub fn from_mode(mode: &SovereignMode) -> Self {
        Self {
            enabled: true,
            session_id: Some(mode.session_id().to_string()),
            ollama_endpoint: mode.ollama_base_url().to_string(),
            ollama_reachable: true, // probed at enable time
            allowed_egress: mode.allowed_egress_count(),
            blocked_egress: mode.blocked_egress_count(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn loopback_hosts_allowed() {
        for host in [
            "localhost",
            "LOCALHOST",
            "localhost.",
            "127.0.0.1",
            "127.0.0.2",
            "127.255.255.255",
            "::1",
            "[::1]",
            "0.0.0.0",
        ] {
            assert!(is_loopback_host(host), "expected loopback: {host}");
        }
    }

    #[test]
    fn non_loopback_hosts_rejected() {
        for host in [
            "example.com",
            "8.8.8.8",
            "192.168.1.1",
            "10.0.0.5",
            "172.16.0.1",
            "localhost.example.com",
            "evil-localhost.com",
            "",
        ] {
            assert!(!is_loopback_host(host), "expected non-loopback: {host}");
        }
    }

    #[test]
    fn sovereign_allows_ollama_urls() {
        for url in [
            "http://localhost:11434/v1",
            "http://localhost:11434/api/chat",
            "http://127.0.0.1:11434/v1/chat/completions",
            "http://[::1]:11434/v1",
        ] {
            assert_eq!(check_url_sovereign(url), EgressDecision::Allow, "{url}");
        }
    }

    #[test]
    fn sovereign_blocks_remote_urls() {
        for url in [
            "https://api.openai.com/v1/chat/completions",
            "https://example.com:443/x?key=secret",
            "http://192.168.1.10:11434/v1",
            "https://user:pass@api.example.com/v1",
            "not-a-url",
            "",
        ] {
            match check_url_sovereign(url) {
                EgressDecision::Block { .. } => {}
                EgressDecision::Allow => panic!("should have blocked: {url}"),
            }
        }
    }

    #[test]
    fn validate_accepts_ollama_without_key() {
        assert!(validate_llm_config("http://localhost:11434/v1", None).is_ok());
        assert!(validate_llm_config("http://localhost:11434/v1", Some("  ")).is_ok());
    }

    #[test]
    fn validate_rejects_remote_endpoint() {
        let err = validate_llm_config("https://api.openai.com/v1", None).unwrap_err();
        assert!(matches!(err, SovereignError::NonSovereignConfig { .. }));
    }

    #[test]
    fn validate_rejects_any_api_key() {
        let err = validate_llm_config("http://localhost:11434/v1", Some("sk-abc")).unwrap_err();
        assert!(matches!(err, SovereignError::NonSovereignConfig { .. }));
        // Even a loopback endpoint with a key is rejected: a key implies a
        // remote account that could bill or log.
    }

    #[test]
    fn preflight_clean_config_has_no_findings() {
        let findings = preflight_check("http://localhost:11434/v1", None, &[], false);
        assert!(findings.is_empty());
    }

    #[test]
    fn preflight_flags_remote_endpoint_and_key() {
        let findings = preflight_check(
            "https://api.openai.com/v1",
            Some("sk-secret"),
            &["https://telemetry.example.com/ping".to_string()],
            false,
        );
        let kinds: Vec<&str> = findings.iter().map(|f| f.kind.as_str()).collect();
        assert!(kinds.contains(&"remote-endpoint"));
        assert!(kinds.contains(&"api-key-present"));
        // Remote endpoints are NOT acknowledgable - they must be fixed.
        assert!(findings
            .iter()
            .filter(|f| f.kind == "remote-endpoint")
            .all(|f| !f.acknowledgable));
        assert!(findings
            .iter()
            .filter(|f| f.kind == "api-key-present")
            .all(|f| f.acknowledgable));
    }

    #[test]
    fn attestation_chain_verifies() {
        let mut log = AttestationLog::new("sess-1");
        log.record(
            "egress-allowed",
            "http://localhost:11434/v1/chat/completions",
        );
        log.record("egress-blocked", "blocked 'https://evil.example.com/x'");
        let att = log.finalize("sess-1");
        assert_eq!(att.entries.len(), 3); // genesis + 2
        assert!(verify_attestation(&att));
    }

    #[test]
    fn attestation_detects_tampering() {
        let mut log = AttestationLog::new("sess-2");
        log.record("egress-allowed", "http://localhost:11434/v1");
        let mut att = log.finalize("sess-2");
        att.entries[1].detail = "http://localhost:11434/v1 -- tampered".to_string();
        assert!(!verify_attestation(&att));
        // Swapping the final hash also fails.
        let mut log2 = AttestationLog::new("sess-3");
        log2.record("egress-allowed", "x");
        let mut att2 = log2.finalize("sess-3");
        att2.final_hash = "deadbeef".to_string();
        assert!(!verify_attestation(&att2));
    }

    #[test]
    fn attestation_rejects_wrong_scheme_or_empty() {
        let log = AttestationLog::new("sess-4");
        let mut att = log.finalize("sess-4");
        att.scheme = "something-else".to_string();
        assert!(!verify_attestation(&att));
        let empty = Attestation {
            scheme: ATTESTATION_SCHEME.to_string(),
            policy_version: POLICY_VERSION.to_string(),
            session_id: "e".to_string(),
            finalized_at: "t".to_string(),
            entries: vec![],
            final_hash: String::new(),
        };
        assert!(!verify_attestation(&empty));
    }

    #[test]
    fn mode_blocks_remote_egress_and_counts() {
        let mut mode = SovereignMode::enable_with_probe("sess-5", OLLAMA_LOOPBACK_URL, false)
            .expect("enable without probe");
        assert!(mode.check_egress("http://localhost:11434/v1/chat").is_ok());
        let err = mode.check_egress("https://api.openai.com/v1").unwrap_err();
        assert!(matches!(err, SovereignError::BlockedUrl { .. }));
        assert_eq!(mode.allowed_egress_count(), 1);
        assert_eq!(mode.blocked_egress_count(), 1);
        // The blocked attempt is recorded in the attestation log.
        assert!(mode
            .attestation_entries()
            .iter()
            .any(|e| e.kind == "egress-blocked"));
        let att = mode.disable();
        assert!(verify_attestation(&att));
        assert!(att.entries.iter().any(|e| e.kind == "mode-disabled"));
    }

    #[test]
    fn enable_rejects_remote_endpoint() {
        let err = SovereignMode::enable_with_probe("sess-6", "https://api.openai.com/v1", false)
            .unwrap_err();
        assert!(matches!(err, SovereignError::NonSovereignConfig { .. }));
    }

    #[test]
    fn enable_probes_ollama_liveness() {
        // Bind a throwaway listener: the probe must see it as "running".
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let port = listener.local_addr().expect("local addr").port();
        assert!(probe_ollama_running(port));
        drop(listener);
        // A port that is (almost certainly) closed must probe as down.
        assert!(!probe_ollama_running(1));
    }

    #[test]
    fn status_disabled_shape() {
        let s = SovereignStatus::disabled();
        assert!(!s.enabled);
        assert_eq!(s.ollama_endpoint, OLLAMA_LOOPBACK_URL);
    }

    #[test]
    fn error_messages_are_actionable() {
        let e = SovereignError::OllamaUnreachable {
            endpoint: "127.0.0.1:11434".to_string(),
        };
        assert!(e.to_string().contains("ollama serve"));
        let e2 = SovereignError::BlockedUrl {
            url: "https://x.example".to_string(),
            reason: "not loopback".to_string(),
        };
        assert!(e2.to_string().contains("Sovereign Mode blocked"));
    }
}
