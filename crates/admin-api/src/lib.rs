use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use argon2::Argon2;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use axum::extract::{Path, Query, State};
use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use rand::RngExt;
use serde::Deserialize;
use std::str::FromStr;
use thiserror::Error;
use time::{Duration, OffsetDateTime};
use tracing::{error, info};
use uuid::Uuid;
use wdapm_core::{
    AccountHealthReport, AdminAuditLogInsert, AdminAuditLogRecord, AlertEventRecord,
    AlertRuleInsert, AlertRuleKind, AlertRuleRecord, EgressProxy, EgressProxyKind,
    EgressProxyStatus, EgressProxySummary, PlatformApiKeyRecord, ProviderAccount,
    ProviderAccountStatus, ProviderAccountSummary, ProviderAccountUpdate, ProviderAsyncJobRecord,
    ProviderAsyncJobState, ProviderId, ProviderRequestReport, RequestLogRecord, hash_token,
    parse_sqlite_timestamp, sqlite_timestamp,
};
use wdapm_storage::{PlatformApiKeyInsert, StorageError, StorageService};
use wdapm_worker::{EgressProxyTestResult, ReconcileReport, WorkerError, WorkerService};

const ADMIN_SESSION_TTL_SECONDS: i64 = 86_400;

#[derive(Clone)]
pub struct AdminApiState {
    default_tenant_id: Arc<String>,
    storage: Arc<StorageService>,
    worker: Arc<WorkerService>,
}

impl AdminApiState {
    pub fn new(
        default_tenant_id: String,
        storage: Arc<StorageService>,
        worker: Arc<WorkerService>,
    ) -> Self {
        Self {
            default_tenant_id: Arc::new(default_tenant_id),
            storage,
            worker,
        }
    }
}

pub fn build_router(state: AdminApiState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/auth/status", get(auth_status))
        .route("/auth/setup", post(auth_setup))
        .route("/auth/login", post(auth_login))
        .route(
            "/settings/platform-api-keys",
            get(list_platform_api_keys_handler).post(create_platform_api_key_handler),
        )
        .route(
            "/settings/platform-api-keys/{key_id}",
            patch(update_platform_api_key_handler),
        )
        .route(
            "/settings/platform-api-keys/{key_id}/secret",
            get(get_platform_api_key_secret_handler),
        )
        .route(
            "/settings/platform-api-keys/{key_id}/revoke",
            post(revoke_platform_api_key_handler),
        )
        .route(
            "/provider-accounts",
            get(list_provider_accounts).post(create_provider_account),
        )
        .route(
            "/provider-accounts/{account_id}",
            patch(update_provider_account).delete(delete_provider_account),
        )
        .route(
            "/provider-accounts/{account_id}/enable",
            post(enable_provider_account),
        )
        .route(
            "/provider-accounts/{account_id}/disable",
            post(disable_provider_account),
        )
        .route(
            "/provider-accounts/{account_id}/egress-proxies",
            get(list_bound_egress_proxies),
        )
        .route(
            "/provider-accounts/{account_id}/bind-proxy",
            post(bind_provider_account_proxy),
        )
        .route(
            "/egress-proxies",
            get(list_egress_proxies).post(create_egress_proxy),
        )
        .route("/egress-proxies/{proxy_id}/test", post(test_egress_proxy))
        .route("/egress-proxies/{proxy_id}", patch(update_egress_proxy))
        .route("/request-logs", get(list_request_logs))
        .route("/async-jobs", get(list_async_jobs))
        .route(
            "/async-jobs/reconcile/firecrawl",
            post(reconcile_firecrawl_jobs),
        )
        .route("/audit-logs", get(list_audit_logs))
        .route(
            "/alert-rules",
            get(list_alert_rules).post(create_alert_rule),
        )
        .route(
            "/alert-rules/{rule_id}",
            patch(update_alert_rule).delete(delete_alert_rule),
        )
        .route("/alert-events", get(list_alert_events))
        .route(
            "/reports/requests-by-provider",
            get(report_requests_by_provider),
        )
        .route("/reports/account-health", get(report_account_health))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

include!("provider_accounts.rs");

async fn list_egress_proxies(
    State(state): State<AdminApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<EgressProxySummary>>, AdminApiError> {
    authorize(&headers, &state.storage).await?;
    let proxies = state.storage.list_egress_proxies().await?;
    Ok(Json(proxies))
}

include!("egress_proxies.rs");

async fn list_request_logs(
    State(state): State<AdminApiState>,
    headers: HeaderMap,
    Query(query): Query<ListRequestLogsQuery>,
) -> Result<Json<Vec<RequestLogRecord>>, AdminApiError> {
    authorize(&headers, &state.storage).await?;
    let filter = wdapm_storage::RequestLogFilter {
        provider: query.provider,
        status_min: query.status_min,
        status_max: query.status_max,
        latency_min: query.latency_min,
        latency_max: query.latency_max,
        since: query.since,
        until: query.until,
        api_key_name: query.api_key_name,
        limit: query.limit,
    };
    let logs = state.storage.list_request_logs_filtered(&filter).await?;
    Ok(Json(logs))
}

async fn list_async_jobs(
    State(state): State<AdminApiState>,
    headers: HeaderMap,
    Query(query): Query<ListAsyncJobsQuery>,
) -> Result<Json<Vec<ProviderAsyncJobRecord>>, AdminApiError> {
    authorize(&headers, &state.storage).await?;
    let jobs = state
        .storage
        .list_provider_async_jobs(query.provider, query.state, query.limit.unwrap_or(100))
        .await?;
    Ok(Json(jobs))
}

async fn reconcile_firecrawl_jobs(
    State(state): State<AdminApiState>,
    headers: HeaderMap,
    payload: Option<Json<ReconcileFirecrawlJobsRequest>>,
) -> Result<Json<ReconcileReport>, AdminApiError> {
    authorize(&headers, &state.storage).await?;
    let report = state
        .worker
        .reconcile_firecrawl_jobs(
            payload
                .map(|Json(value)| value.limit.unwrap_or(50))
                .unwrap_or(50),
        )
        .await?;
    Ok(Json(report))
}

fn generate_random_token() -> String {
    let bytes: [u8; 32] = rand::rng().random();
    hex::encode(bytes)
}

fn generate_platform_api_key() -> String {
    let bytes: [u8; 24] = rand::rng().random();
    format!("wdapm_{}", hex::encode(bytes))
}

fn session_expires_at() -> String {
    sqlite_timestamp(OffsetDateTime::now_utc() + Duration::seconds(ADMIN_SESSION_TTL_SECONDS))
}

include!("auth.rs");
include!("platform_api_keys.rs");

async fn authorize(headers: &HeaderMap, storage: &StorageService) -> Result<(), AdminApiError> {
    authorize_with_identity(headers, storage).await.map(|_| ())
}

async fn authorize_with_identity(
    headers: &HeaderMap,
    storage: &StorageService,
) -> Result<String, AdminApiError> {
    let value = headers
        .get(AUTHORIZATION)
        .ok_or(AdminApiError::MissingAuthorization)?;
    let raw = value
        .to_str()
        .map_err(|_| AdminApiError::InvalidAuthorization)?;
    let token = raw
        .strip_prefix("Bearer ")
        .or_else(|| raw.strip_prefix("bearer "))
        .ok_or(AdminApiError::InvalidAuthorization)?;

    let stored_token_hash = storage
        .get_admin_config("session_token_hash")
        .await
        .map_err(AdminApiError::Storage)?;
    let session_expires_at = storage
        .get_admin_config("session_expires_at")
        .await
        .map_err(AdminApiError::Storage)?;
    let token_hash = stored_token_hash.ok_or(AdminApiError::InvalidAdminSession)?;
    let expires_at = session_expires_at
        .as_deref()
        .and_then(parse_sqlite_timestamp)
        .ok_or(AdminApiError::InvalidAdminSession)?;
    if OffsetDateTime::now_utc() > expires_at {
        return Err(AdminApiError::InvalidAdminSession);
    }
    if hash_token(token) != token_hash {
        return Err(AdminApiError::InvalidAdminSession);
    }
    let identity = if token.len() > 8 {
        format!("{}...", &token[..8])
    } else {
        token.to_owned()
    };
    Ok(identity)
}

async fn emit_audit(
    state: &AdminApiState,
    admin_identity: &str,
    action: &str,
    resource_type: &str,
    resource_id: Option<&str>,
    old_value: Option<serde_json::Value>,
    new_value: Option<serde_json::Value>,
) {
    let entry = AdminAuditLogInsert {
        id: Uuid::now_v7(),
        admin_identity: admin_identity.to_owned(),
        action: action.to_owned(),
        resource_type: resource_type.to_owned(),
        resource_id: resource_id.map(ToOwned::to_owned),
        old_value,
        new_value,
    };
    if let Err(err) = state.storage.insert_admin_audit_log(&entry).await {
        error!(%err, action, resource_type, "failed to insert admin audit log");
    }
}

async fn list_audit_logs(
    State(state): State<AdminApiState>,
    headers: HeaderMap,
    Query(query): Query<AuditLogListQuery>,
) -> Result<Json<Vec<AdminAuditLogRecord>>, AdminApiError> {
    authorize(&headers, &state.storage).await?;
    let logs = state
        .storage
        .list_admin_audit_logs(
            query.since.as_deref(),
            query.until.as_deref(),
            query.resource_type.as_deref(),
            query.limit.unwrap_or(100),
        )
        .await?;
    Ok(Json(logs))
}

async fn list_alert_rules(
    State(state): State<AdminApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<AlertRuleRecord>>, AdminApiError> {
    authorize(&headers, &state.storage).await?;
    let rules = state.storage.list_alert_rules().await?;
    Ok(Json(rules))
}

async fn create_alert_rule(
    State(state): State<AdminApiState>,
    headers: HeaderMap,
    Json(payload): Json<CreateAlertRuleRequest>,
) -> Result<(StatusCode, Json<AlertRuleRecord>), AdminApiError> {
    let admin_identity = authorize_with_identity(&headers, &state.storage).await?;
    let name = require_non_empty(payload.name.as_str(), "alert rule name is required")?;
    let kind = parse_alert_rule_kind(payload.kind.as_str())?;
    let threshold_value = validate_positive(
        payload.threshold_value,
        "alert rule threshold_value must be greater than 0",
    )?;
    let webhook_url = normalize_webhook_url(payload.webhook_url.as_str())?;
    let id = Uuid::now_v7();
    let insert = AlertRuleInsert {
        id,
        name,
        kind,
        threshold_value,
        webhook_url,
        enabled: payload.enabled.unwrap_or(true),
    };
    state.storage.insert_alert_rule(&insert).await?;
    let record = state
        .storage
        .find_alert_rule(&id.to_string())
        .await?
        .ok_or_else(|| AdminApiError::NotFound("alert rule".to_owned()))?;
    emit_audit(
        &state,
        &admin_identity,
        "create",
        "alert_rule",
        Some(&record.id),
        None,
        Some(serde_json::json!({
            "name": record.name,
            "kind": record.kind,
        })),
    )
    .await;
    Ok((StatusCode::CREATED, Json(record)))
}

async fn update_alert_rule(
    State(state): State<AdminApiState>,
    headers: HeaderMap,
    Path(rule_id): Path<String>,
    Json(payload): Json<UpdateAlertRuleRequest>,
) -> Result<Json<AlertRuleRecord>, AdminApiError> {
    let admin_identity = authorize_with_identity(&headers, &state.storage).await?;
    let existing = state
        .storage
        .find_alert_rule(&rule_id)
        .await?
        .ok_or_else(|| AdminApiError::NotFound(format!("alert rule `{rule_id}`")))?;
    let name = payload
        .name
        .as_ref()
        .map(|value| require_non_empty(value.as_str(), "alert rule name cannot be empty"))
        .transpose()?
        .unwrap_or(existing.name.clone());
    let kind = payload
        .kind
        .as_ref()
        .map(|value| parse_alert_rule_kind(value.as_str()))
        .transpose()?
        .unwrap_or(existing.kind);
    let threshold_value = validate_positive(
        payload.threshold_value.unwrap_or(existing.threshold_value),
        "alert rule threshold_value must be greater than 0",
    )?;
    let webhook_url = payload
        .webhook_url
        .as_ref()
        .map(|value| normalize_webhook_url(value.as_str()))
        .transpose()?
        .unwrap_or(existing.webhook_url.clone());
    let enabled = payload.enabled.unwrap_or(existing.enabled);
    let updated = state
        .storage
        .update_alert_rule(
            &rule_id,
            &name,
            kind,
            threshold_value,
            &webhook_url,
            enabled,
        )
        .await?;
    if !updated {
        return Err(AdminApiError::NotFound(format!("alert rule `{rule_id}`")));
    }
    let record = state
        .storage
        .find_alert_rule(&rule_id)
        .await?
        .ok_or_else(|| AdminApiError::NotFound(format!("alert rule `{rule_id}`")))?;
    emit_audit(
        &state,
        &admin_identity,
        "update",
        "alert_rule",
        Some(&rule_id),
        Some(serde_json::json!({
            "name": existing.name,
            "kind": existing.kind,
            "threshold_value": existing.threshold_value,
        })),
        Some(serde_json::json!({
            "name": record.name,
            "kind": record.kind,
            "threshold_value": record.threshold_value,
        })),
    )
    .await;
    Ok(Json(record))
}

async fn delete_alert_rule(
    State(state): State<AdminApiState>,
    headers: HeaderMap,
    Path(rule_id): Path<String>,
) -> Result<StatusCode, AdminApiError> {
    let admin_identity = authorize_with_identity(&headers, &state.storage).await?;
    let deleted = state.storage.delete_alert_rule(&rule_id).await?;
    if !deleted {
        return Err(AdminApiError::NotFound(format!("alert rule `{rule_id}`")));
    }
    emit_audit(
        &state,
        &admin_identity,
        "delete",
        "alert_rule",
        Some(&rule_id),
        None,
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_alert_events(
    State(state): State<AdminApiState>,
    headers: HeaderMap,
    Query(query): Query<AlertEventListQuery>,
) -> Result<Json<Vec<AlertEventRecord>>, AdminApiError> {
    authorize(&headers, &state.storage).await?;
    let events = state
        .storage
        .list_alert_events(
            query.since.as_deref(),
            query.until.as_deref(),
            query.kind.as_deref(),
            query.limit.unwrap_or(100),
        )
        .await?;
    Ok(Json(events))
}

async fn report_requests_by_provider(
    State(state): State<AdminApiState>,
    headers: HeaderMap,
    Query(query): Query<ReportTimeRangeQuery>,
) -> Result<Json<Vec<ProviderRequestReport>>, AdminApiError> {
    authorize(&headers, &state.storage).await?;
    let report = state
        .storage
        .report_requests_by_provider(query.since.as_deref(), query.until.as_deref())
        .await?;
    Ok(Json(report))
}

async fn report_account_health(
    State(state): State<AdminApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<AccountHealthReport>>, AdminApiError> {
    authorize(&headers, &state.storage).await?;
    let report = state.storage.report_account_health().await?;
    Ok(Json(report))
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    })
}

fn normalize_text(value: String) -> Option<String> {
    normalize_optional_text(Some(value))
}

fn require_non_empty(value: &str, message: &str) -> Result<String, AdminApiError> {
    let value = value.trim();
    if value.is_empty() {
        Err(AdminApiError::BadRequest(message.to_owned()))
    } else {
        Ok(value.to_owned())
    }
}

fn optional_non_empty(
    value: Option<&String>,
    message: &str,
) -> Result<Option<String>, AdminApiError> {
    value
        .map(|value| require_non_empty(value.as_str(), message))
        .transpose()
}

fn validate_non_negative(value: i64, message: &str) -> Result<i64, AdminApiError> {
    if value < 0 {
        Err(AdminApiError::BadRequest(message.to_owned()))
    } else {
        Ok(value)
    }
}

fn validate_positive(value: i64, message: &str) -> Result<i64, AdminApiError> {
    if value <= 0 {
        Err(AdminApiError::BadRequest(message.to_owned()))
    } else {
        Ok(value)
    }
}

fn parse_alert_rule_kind(value: &str) -> Result<AlertRuleKind, AdminApiError> {
    let value = require_non_empty(value, "alert rule kind is required")?;
    AlertRuleKind::from_str(value.as_str())
        .map_err(|error| AdminApiError::BadRequest(error.to_string()))
}

fn normalize_webhook_url(value: &str) -> Result<String, AdminApiError> {
    let value = require_non_empty(value, "alert rule webhook_url is required")?;
    let uri = value
        .parse::<Uri>()
        .map_err(|_| AdminApiError::BadRequest("alert rule webhook_url is invalid".to_owned()))?;
    let Some(scheme) = uri.scheme_str() else {
        return Err(AdminApiError::BadRequest(
            "alert rule webhook_url must be absolute".to_owned(),
        ));
    };
    if !matches!(scheme, "http" | "https") || uri.host().is_none() {
        return Err(AdminApiError::BadRequest(
            "alert rule webhook_url must use http or https".to_owned(),
        ));
    }
    Ok(value)
}

fn generate_account_id(provider: ProviderId, name: &str) -> String {
    generate_named_id(provider.as_str(), name)
}

fn generate_named_id(prefix: &str, name: &str) -> String {
    let slug = name
        .trim()
        .chars()
        .map(|value| {
            if value.is_ascii_alphanumeric() {
                value.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned();
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);

    if slug.is_empty() {
        format!("{prefix}-{suffix}")
    } else {
        format!("{prefix}-{slug}-{suffix}")
    }
}

#[derive(Debug, serde::Serialize)]
struct AuthStatusResponse {
    initialized: bool,
}

#[derive(Debug, Deserialize)]
struct AuthSetupRequest {
    password: String,
}

#[derive(Debug, Deserialize)]
struct AuthLoginRequest {
    password: String,
}

#[derive(Debug, serde::Serialize)]
struct AuthTokenResponse {
    token: String,
}

#[derive(Debug, serde::Serialize)]
struct AuthSetupResponse {
    token: String,
}

#[derive(Debug, Deserialize)]
struct CreatePlatformApiKeyRequest {
    name: String,
    quota: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct UpdatePlatformApiKeyRequest {
    name: Option<String>,
    quota: Option<i64>,
}

#[derive(Debug, serde::Serialize)]
struct CreatePlatformApiKeyResponse {
    id: String,
    name: String,
    key: String,
    key_prefix: String,
    quota: i64,
}

#[derive(Debug, serde::Serialize)]
struct RevealPlatformApiKeyResponse {
    key: String,
}

#[derive(Debug, Deserialize)]
struct ProviderAccountListQuery {
    provider: Option<ProviderId>,
}

#[derive(Debug, Deserialize)]
struct ListRequestLogsQuery {
    provider: Option<String>,
    status_min: Option<i64>,
    status_max: Option<i64>,
    latency_min: Option<i64>,
    latency_max: Option<i64>,
    since: Option<String>,
    until: Option<String>,
    api_key_name: Option<String>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ListAsyncJobsQuery {
    provider: Option<ProviderId>,
    state: Option<ProviderAsyncJobState>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct CreateProviderAccountRequest {
    id: Option<String>,
    provider: ProviderId,
    name: String,
    api_key: String,
    base_url: Option<String>,
    reader_base_url: Option<String>,
    search_base_url: Option<String>,
    enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct UpdateProviderAccountRequest {
    name: Option<String>,
    api_key: Option<String>,
    base_url: Option<String>,
    clear_base_url: Option<bool>,
    reader_base_url: Option<String>,
    clear_reader_base_url: Option<bool>,
    search_base_url: Option<String>,
    clear_search_base_url: Option<bool>,
    enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct BindProxyRequest {
    egress_proxy_id: String,
}

#[derive(Debug, Deserialize)]
struct CreateEgressProxyRequest {
    id: Option<String>,
    name: String,
    proxy_url: String,
    region: Option<String>,
    enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct UpdateEgressProxyRequest {
    name: Option<String>,
    proxy_url: Option<String>,
    region: Option<String>,
    clear_region: Option<bool>,
    enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ReconcileFirecrawlJobsRequest {
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct AuditLogListQuery {
    since: Option<String>,
    until: Option<String>,
    resource_type: Option<String>,
    limit: Option<i64>,
}

#[derive(Deserialize)]
struct CreateAlertRuleRequest {
    name: String,
    kind: String,
    threshold_value: i64,
    webhook_url: String,
    enabled: Option<bool>,
}

#[derive(Deserialize)]
struct UpdateAlertRuleRequest {
    name: Option<String>,
    kind: Option<String>,
    threshold_value: Option<i64>,
    webhook_url: Option<String>,
    enabled: Option<bool>,
}

#[derive(Deserialize)]
struct AlertEventListQuery {
    since: Option<String>,
    until: Option<String>,
    kind: Option<String>,
    limit: Option<i64>,
}

#[derive(Deserialize)]
struct ReportTimeRangeQuery {
    since: Option<String>,
    until: Option<String>,
}

#[derive(Debug, serde::Serialize)]
struct ProviderAccountToggleResponse {
    id: String,
    enabled: bool,
}

#[derive(Debug, Error)]
pub enum AdminApiError {
    #[error("missing admin authorization header")]
    MissingAuthorization,
    #[error("admin authorization header is invalid")]
    InvalidAuthorization,
    #[error("admin credentials are invalid")]
    InvalidAdminCredentials,
    #[error("admin session is invalid")]
    InvalidAdminSession,
    #[error("{0}")]
    BadRequest(String),
    #[error("resource `{0}` was not found")]
    NotFound(String),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Worker(#[from] WorkerError),
}

impl IntoResponse for AdminApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::MissingAuthorization
            | Self::InvalidAuthorization
            | Self::InvalidAdminCredentials
            | Self::InvalidAdminSession => (StatusCode::UNAUTHORIZED, self.to_string()),
            Self::BadRequest(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            Self::NotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            Self::Storage(ref err) => {
                error!(%err, "admin api storage error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".to_owned(),
                )
            }
            Self::Worker(ref err) => {
                error!(%err, "admin api worker error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".to_owned(),
                )
            }
        };

        (status, message).into_response()
    }
}
