use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

pub type HeaderValues = BTreeMap<String, String>;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    Exa,
    Tavily,
    Firecrawl,
    Jina,
}

impl ProviderId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Exa => "exa",
            Self::Tavily => "tavily",
            Self::Firecrawl => "firecrawl",
            Self::Jina => "jina",
        }
    }

    pub const fn default_base_url(self) -> &'static str {
        match self {
            Self::Exa => "https://api.exa.ai",
            Self::Tavily => "https://api.tavily.com",
            Self::Firecrawl => "https://api.firecrawl.dev",
            Self::Jina => "https://r.jina.ai",
        }
    }
}

impl Display for ProviderId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ProviderId {
    type Err = ProviderError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "exa" => Ok(Self::Exa),
            "tavily" => Ok(Self::Tavily),
            "firecrawl" => Ok(Self::Firecrawl),
            "jina" => Ok(Self::Jina),
            _ => Err(ProviderError::UnsupportedProvider(value.to_owned())),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RequestEnvelope {
    pub request_id: Uuid,
    pub method: String,
    pub rest_path: String,
    pub query: Option<String>,
    pub headers: HeaderValues,
    pub body: Vec<u8>,
    pub received_at: OffsetDateTime,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProviderAccount {
    pub id: String,
    pub provider: ProviderId,
    pub name: String,
    pub api_key: String,
    pub base_url: Option<String>,
    pub reader_base_url: Option<String>,
    pub search_base_url: Option<String>,
    pub enabled: bool,
    pub status: ProviderAccountStatus,
    pub last_error: Option<String>,
    pub cooldown_until: Option<String>,
    pub last_used_at: Option<String>,
    pub consecutive_failures: i64,
    pub last_status_code: Option<i64>,
    pub weight: i64,
    pub last_failure_at: Option<String>,
}

impl ProviderAccount {
    pub fn base_url(&self) -> &str {
        self.base_url
            .as_deref()
            .unwrap_or_else(|| self.provider.default_base_url())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAccountStatus {
    Active,
    Cooldown,
    Disabled,
}

impl ProviderAccountStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Cooldown => "cooldown",
            Self::Disabled => "disabled",
        }
    }
}

impl FromStr for ProviderAccountStatus {
    type Err = ProviderError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "active" => Ok(Self::Active),
            "cooldown" => Ok(Self::Cooldown),
            "disabled" => Ok(Self::Disabled),
            _ => Err(ProviderError::InvalidRoute(format!(
                "invalid provider account status `{value}`"
            ))),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProviderAccountSummary {
    pub id: String,
    pub provider: ProviderId,
    pub name: String,
    pub base_url: Option<String>,
    pub reader_base_url: Option<String>,
    pub search_base_url: Option<String>,
    pub enabled: bool,
    pub status: ProviderAccountStatus,
    pub last_error: Option<String>,
    pub cooldown_until: Option<String>,
    pub last_used_at: Option<String>,
    pub consecutive_failures: i64,
    pub last_status_code: Option<i64>,
    pub weight: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Default)]
pub struct ProviderAccountUpdate {
    pub name: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<Option<String>>,
    pub reader_base_url: Option<Option<String>>,
    pub search_base_url: Option<Option<String>>,
    pub enabled: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EgressProxy {
    pub id: String,
    pub name: String,
    pub kind: EgressProxyKind,
    pub proxy_url: String,
    pub region: Option<String>,
    pub enabled: bool,
    pub status: EgressProxyStatus,
    pub last_error: Option<String>,
    pub cooldown_until: Option<String>,
    pub last_used_at: Option<String>,
    pub consecutive_failures: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EgressProxySummary {
    pub id: String,
    pub name: String,
    pub kind: EgressProxyKind,
    pub proxy_url: String,
    pub region: Option<String>,
    pub enabled: bool,
    pub status: EgressProxyStatus,
    pub last_error: Option<String>,
    pub cooldown_until: Option<String>,
    pub last_used_at: Option<String>,
    pub consecutive_failures: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SchedulerSelection {
    pub account: ProviderAccount,
    pub egress_proxy: Option<EgressProxy>,
    pub selection_reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RequestLogRecord {
    pub id: String,
    pub tenant_id: Option<String>,
    pub platform_api_key_id: Option<String>,
    pub provider: ProviderId,
    pub provider_account_id: Option<String>,
    pub egress_proxy_id: Option<String>,
    pub method: String,
    pub route: String,
    pub upstream_url: String,
    pub status_code: Option<i64>,
    pub latency_ms: Option<i64>,
    pub failure_kind: Option<String>,
    pub failure_message: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PlatformApiKeyRecord {
    pub id: String,
    pub name: String,
    pub key_prefix: String,
    pub quota: i64,
    pub request_count: i64,
    pub created_at: String,
    pub revoked_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProviderAsyncJobRecord {
    pub id: String,
    pub tenant_id: Option<String>,
    pub request_log_id: Option<String>,
    pub provider: ProviderId,
    pub provider_account_id: Option<String>,
    pub egress_proxy_id: Option<String>,
    pub route: String,
    pub upstream_job_id: String,
    pub state: ProviderAsyncJobState,
    pub last_status_code: Option<i64>,
    pub last_error: Option<String>,
    pub poll_attempts: i64,
    pub next_poll_at: Option<String>,
    pub settled_at: Option<String>,
    pub metadata: serde_json::Value,
    pub webhook_secret: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProviderAsyncJobInsert {
    pub id: Uuid,
    pub tenant_id: Option<String>,
    pub request_log_id: Option<Uuid>,
    pub provider: ProviderId,
    pub provider_account_id: Option<String>,
    pub egress_proxy_id: Option<String>,
    pub route: String,
    pub upstream_job_id: String,
    pub state: ProviderAsyncJobState,
    pub last_status_code: Option<i64>,
    pub last_error: Option<String>,
    pub poll_attempts: i64,
    pub next_poll_at: Option<String>,
    pub settled_at: Option<String>,
    pub metadata: serde_json::Value,
    pub webhook_secret: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProviderAsyncJobUpdate {
    pub state: ProviderAsyncJobState,
    pub status_code: Option<i64>,
    pub last_error: Option<String>,
    pub poll_attempt_increment: i64,
    pub next_poll_at: Option<String>,
    pub settled_at: Option<String>,
    pub metadata: serde_json::Value,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EgressProxyKind {
    Http,
    Https,
    Socks4,
    Socks5,
    Socks5h,
}

impl EgressProxyKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Https => "https",
            Self::Socks4 => "socks4",
            Self::Socks5 => "socks5",
            Self::Socks5h => "socks5h",
        }
    }

    pub fn from_proxy_url(value: &str) -> Result<Self, ProviderError> {
        let scheme = value
            .split_once("://")
            .map(|(scheme, _)| scheme)
            .ok_or_else(|| ProviderError::InvalidTarget(format!("invalid proxy url `{value}`")))?;
        Self::from_str(scheme.to_ascii_lowercase().as_str())
    }
}

impl FromStr for EgressProxyKind {
    type Err = ProviderError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "http" => Ok(Self::Http),
            "https" => Ok(Self::Https),
            "socks4" => Ok(Self::Socks4),
            "socks5" => Ok(Self::Socks5),
            "socks5h" => Ok(Self::Socks5h),
            _ => Err(ProviderError::InvalidTarget(format!(
                "unsupported proxy scheme `{value}`"
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EgressProxyStatus {
    Active,
    Cooldown,
    Disabled,
}

impl EgressProxyStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Cooldown => "cooldown",
            Self::Disabled => "disabled",
        }
    }
}

impl FromStr for EgressProxyStatus {
    type Err = ProviderError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "active" => Ok(Self::Active),
            "cooldown" => Ok(Self::Cooldown),
            "disabled" => Ok(Self::Disabled),
            _ => Err(ProviderError::InvalidTarget(format!(
                "invalid egress proxy status `{value}`"
            ))),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProviderRoute {
    pub base_url_override: Option<String>,
    pub upstream_path: String,
    pub query: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum ProviderAuth {
    Bearer(String),
    Header { name: String, value: String },
    None,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UpstreamRequestPlan {
    pub provider: ProviderId,
    pub url: String,
    pub auth: ProviderAuth,
    pub body_override: Option<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAsyncJobState {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl ProviderAsyncJobState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

impl FromStr for ProviderAsyncJobState {
    type Err = ProviderError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(ProviderError::InvalidRoute(format!(
                "invalid provider async job state `{value}`"
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseDisposition {
    Passthrough,
    Cooldown,
    DisableAccount,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderResponseClass {
    pub retryable: bool,
    pub disposition: ResponseDisposition,
}

impl ProviderResponseClass {
    pub const fn passthrough() -> Self {
        Self {
            retryable: false,
            disposition: ResponseDisposition::Passthrough,
        }
    }

    pub const fn retryable() -> Self {
        Self {
            retryable: true,
            disposition: ResponseDisposition::Passthrough,
        }
    }

    pub const fn cooldown() -> Self {
        Self {
            retryable: true,
            disposition: ResponseDisposition::Cooldown,
        }
    }

    pub const fn disable_account() -> Self {
        Self {
            retryable: false,
            disposition: ResponseDisposition::DisableAccount,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RequestLogInsert {
    pub id: Uuid,
    pub tenant_id: Option<String>,
    pub platform_api_key_id: Option<String>,
    pub provider: ProviderId,
    pub provider_account_id: Option<String>,
    pub egress_proxy_id: Option<String>,
    pub method: String,
    pub route: String,
    pub upstream_url: String,
    pub status_code: Option<i64>,
    pub latency_ms: Option<i64>,
    pub failure_kind: Option<String>,
    pub failure_message: Option<String>,
    pub request_headers: HeaderValues,
    pub response_headers: HeaderValues,
    pub request_body: Option<String>,
    pub response_body: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AdminAuditLogInsert {
    pub id: Uuid,
    pub admin_identity: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub old_value: Option<serde_json::Value>,
    pub new_value: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AdminAuditLogRecord {
    pub id: String,
    pub admin_identity: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub old_value: Option<serde_json::Value>,
    pub new_value: Option<serde_json::Value>,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AlertRuleInsert {
    pub id: Uuid,
    pub name: String,
    pub kind: AlertRuleKind,
    pub threshold_value: i64,
    pub webhook_url: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AlertRuleRecord {
    pub id: String,
    pub name: String,
    pub kind: AlertRuleKind,
    pub threshold_value: i64,
    pub webhook_url: String,
    pub enabled: bool,
    pub last_triggered_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AlertEventInsert {
    pub id: Uuid,
    pub alert_rule_id: Option<String>,
    pub kind: String,
    pub message: String,
    pub metadata: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AlertEventRecord {
    pub id: String,
    pub alert_rule_id: Option<String>,
    pub kind: String,
    pub message: String,
    pub metadata: serde_json::Value,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProviderRequestReport {
    pub provider: String,
    pub total_requests: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub avg_latency_ms: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AccountHealthReport {
    pub id: String,
    pub provider: String,
    pub name: String,
    pub enabled: bool,
    pub status: String,
    pub consecutive_failures: i64,
    pub weight: i64,
    pub last_used_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertRuleKind {
    AccountDisabled,
    HighErrorRate,
    StaleAsyncJob,
}

impl AlertRuleKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AccountDisabled => "account_disabled",
            Self::HighErrorRate => "high_error_rate",
            Self::StaleAsyncJob => "stale_async_job",
        }
    }
}

impl Display for AlertRuleKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AlertRuleKind {
    type Err = ProviderError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "account_disabled" => Ok(Self::AccountDisabled),
            "high_error_rate" => Ok(Self::HighErrorRate),
            "stale_async_job" => Ok(Self::StaleAsyncJob),
            _ => Err(ProviderError::InvalidRoute(format!(
                "invalid alert rule kind `{value}`"
            ))),
        }
    }
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("provider `{0}` is not supported")]
    UnsupportedProvider(String),
    #[error("provider route is invalid: {0}")]
    InvalidRoute(String),
    #[error("provider target is invalid: {0}")]
    InvalidTarget(String),
    #[error("provider request method is not supported: {0}")]
    UnsupportedMethod(String),
}

pub trait ProviderAdapter: Send + Sync {
    fn provider_id(&self) -> ProviderId;
    fn parse_route(
        &self,
        rest_path: &str,
        query: Option<&str>,
    ) -> Result<ProviderRoute, ProviderError>;
    fn build_upstream_request(
        &self,
        request: &RequestEnvelope,
        route: &ProviderRoute,
        account: &ProviderAccount,
    ) -> Result<UpstreamRequestPlan, ProviderError>;
    fn supports_account_for_route(
        &self,
        _route: &ProviderRoute,
        _account: &ProviderAccount,
    ) -> bool {
        true
    }
    fn classify_response(&self, status: u16) -> ProviderResponseClass;
}

pub fn normalize_rest_path(value: &str) -> Result<String, ProviderError> {
    let trimmed = value.trim_matches('/');
    if trimmed.is_empty() {
        return Err(ProviderError::InvalidRoute(
            "missing upstream path".to_owned(),
        ));
    }
    Ok(format!("/{trimmed}"))
}

pub fn join_url(base_url: &str, upstream_path: &str, query: Option<&str>) -> String {
    let base = base_url.trim_end_matches('/');
    let path = upstream_path.trim_start_matches('/');
    match query {
        Some(query) if !query.is_empty() => format!("{base}/{path}?{query}"),
        _ => format!("{base}/{path}"),
    }
}

pub fn calculate_cooldown_seconds(consecutive_failures: i64) -> i64 {
    const BASE_SECS: i64 = 60;
    const MAX_EXPONENT: u32 = 4;
    let exp = (consecutive_failures.max(0) as u32).min(MAX_EXPONENT);
    let base = BASE_SECS.saturating_mul(1i64 << exp);
    let jitter_max = (base / 5).max(1);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as i64;
    let jitter = (nanos % (jitter_max * 2 + 1)) - jitter_max;
    (base + jitter).max(BASE_SECS)
}

pub fn summarize_proxy_url(value: &str) -> String {
    let Some((scheme, remainder)) = value.split_once("://") else {
        return "invalid".to_owned();
    };
    let authority = remainder
        .split_once('/')
        .map(|(prefix, _)| prefix)
        .unwrap_or(remainder);
    let host = authority
        .rsplit_once('@')
        .map(|(_, suffix)| suffix)
        .unwrap_or(authority);
    if host.is_empty() {
        "invalid".to_owned()
    } else {
        format!("{scheme}://{host}")
    }
}

pub fn hash_token(token: &str) -> String {
    let mut hasher = sha2::Sha256::new();
    use sha2::Digest;
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn sqlite_timestamp(value: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        value.year(),
        u8::from(value.month()),
        value.day(),
        value.hour(),
        value.minute(),
        value.second(),
    )
}

pub fn parse_sqlite_timestamp(value: &str) -> Option<OffsetDateTime> {
    let format = time::format_description::parse_borrowed::<3>(
        "[year]-[month]-[day] [hour]:[minute]:[second]",
    )
    .ok()?;
    time::PrimitiveDateTime::parse(value, &format)
        .ok()
        .map(|dt| dt.assume_utc())
}
