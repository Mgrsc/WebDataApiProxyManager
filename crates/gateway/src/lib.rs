use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use axum::Json;
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Query, State};
use axum::http::header::{AUTHORIZATION, CONNECTION, CONTENT_LENGTH, HOST, TRANSFER_ENCODING};
use axum::http::{HeaderMap, HeaderName, Method, StatusCode, Uri, Version};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use reqwest::{Client, Proxy};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;
use tower_http::trace::TraceLayer;
use tracing::{error, info, warn};
use uuid::Uuid;
use wdapm_core::{
    EgressProxy, HeaderValues, PlatformApiKeyRecord, ProviderAccount, ProviderAdapter,
    ProviderAsyncJobInsert, ProviderAsyncJobState, ProviderAuth, ProviderError, ProviderId,
    ProviderResponseClass, RequestEnvelope, RequestLogInsert, ResponseDisposition,
    UpstreamRequestPlan, calculate_cooldown_seconds, hash_token, summarize_proxy_url,
};
use wdapm_provider_firecrawl::{detect_async_job, parse_webhook_payload};
use wdapm_scheduler::{SchedulerExclusions, SchedulerRouteExclusion, SchedulerService};
use wdapm_storage::StorageService;
use wdapm_worker::WorkerService;

const UPSTREAM_CONNECT_TIMEOUT_SECONDS: u64 = 5;
const UPSTREAM_REQUEST_TIMEOUT_SECONDS: u64 = 60;
const UPSTREAM_IDLE_TIMEOUT_SECONDS: u64 = 90;

#[derive(Clone)]
pub struct GatewayConfig {
    pub request_log_capture: RequestLogCaptureConfig,
    pub request_log_writer_capacity: usize,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            request_log_capture: RequestLogCaptureConfig::default(),
            request_log_writer_capacity: 4096,
        }
    }
}

#[derive(Clone)]
pub struct RequestLogCaptureConfig {
    pub mode: RequestLogCaptureMode,
    pub body_max_bytes: usize,
    pub slow_request_threshold_ms: u64,
}

impl Default for RequestLogCaptureConfig {
    fn default() -> Self {
        Self {
            mode: RequestLogCaptureMode::FailuresAndSlow,
            body_max_bytes: 8 * 1024,
            slow_request_threshold_ms: 2_000,
        }
    }
}

#[derive(Clone, Copy)]
pub enum RequestLogCaptureMode {
    MetadataOnly,
    FailuresOnly,
    FailuresAndSlow,
}

#[derive(Clone)]
pub struct GatewayState {
    default_tenant_id: Arc<String>,
    provider_registry: ProviderRegistry,
    storage: Arc<StorageService>,
    scheduler: Arc<SchedulerService>,
    worker: Arc<WorkerService>,
    webhook_base_url: Option<Arc<String>>,
    client_pool: ClientPool,
    request_log_capture: RequestLogCaptureConfig,
    persistence_tx: mpsc::Sender<PersistenceBundle>,
}

impl GatewayState {
    pub fn new(
        default_tenant_id: String,
        provider_registry: ProviderRegistry,
        storage: Arc<StorageService>,
        scheduler: Arc<SchedulerService>,
        worker: Arc<WorkerService>,
        webhook_base_url: Option<String>,
        config: GatewayConfig,
    ) -> Self {
        let (persistence_tx, mut persistence_rx) =
            mpsc::channel::<PersistenceBundle>(config.request_log_writer_capacity.max(64));
        let persistence_storage = storage.clone();
        tokio::spawn(async move {
            while let Some(bundle) = persistence_rx.recv().await {
                if let Err(error) = persist_bundle(&persistence_storage, bundle).await {
                    error!(error = %error, "gateway persistence bundle failed");
                }
            }
        });
        Self {
            default_tenant_id: Arc::new(default_tenant_id),
            provider_registry,
            storage,
            scheduler,
            worker,
            webhook_base_url: webhook_base_url.map(Arc::new),
            client_pool: ClientPool::default(),
            request_log_capture: config.request_log_capture,
            persistence_tx,
        }
    }

    fn try_enqueue_persistence(
        &self,
        bundle: PersistenceBundle,
    ) -> Result<(), Box<mpsc::error::TrySendError<PersistenceBundle>>> {
        self.persistence_tx.try_send(bundle).map_err(Box::new)
    }
}

include!("client_pool.rs");

#[derive(Clone, Default)]
pub struct ProviderRegistry {
    adapters: Arc<std::collections::BTreeMap<ProviderId, Arc<dyn ProviderAdapter>>>,
}

impl ProviderRegistry {
    pub fn builder() -> ProviderRegistryBuilder {
        ProviderRegistryBuilder::default()
    }

    pub fn resolve(&self, provider_id: ProviderId) -> Option<Arc<dyn ProviderAdapter>> {
        self.adapters.get(&provider_id).cloned()
    }
}

#[derive(Default)]
pub struct ProviderRegistryBuilder {
    adapters: std::collections::BTreeMap<ProviderId, Arc<dyn ProviderAdapter>>,
}

impl ProviderRegistryBuilder {
    pub fn register(mut self, adapter: Arc<dyn ProviderAdapter>) -> Self {
        self.adapters.insert(adapter.provider_id(), adapter);
        self
    }

    pub fn build(self) -> ProviderRegistry {
        ProviderRegistry {
            adapters: Arc::new(self.adapters),
        }
    }
}

pub fn build_router(state: GatewayState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/webhooks/firecrawl", post(webhook_firecrawl))
        .route("/exa", any(proxy))
        .route("/exa/{*rest}", any(proxy))
        .route("/tavily", any(proxy))
        .route("/tavily/{*rest}", any(proxy))
        .route("/firecrawl", any(proxy))
        .route("/firecrawl/{*rest}", any(proxy))
        .route("/jina", any(proxy))
        .route("/jina/{*rest}", any(proxy))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health(State(state): State<GatewayState>) -> Json<HealthResponse> {
    let database = match state.storage.check_db_connection().await {
        Ok(()) => "connected".to_owned(),
        Err(err) => {
            error!(error = %err, "health check database connection failed");
            "error".to_owned()
        }
    };
    let providers = match state
        .storage
        .count_active_provider_accounts_by_provider()
        .await
    {
        Ok(counts) => counts
            .into_iter()
            .map(|(provider, count)| {
                (
                    provider,
                    ProviderHealth {
                        active_accounts: count,
                    },
                )
            })
            .collect(),
        Err(err) => {
            error!(error = %err, "health check provider count failed");
            std::collections::BTreeMap::new()
        }
    };
    let status = if database == "connected" {
        "ok"
    } else {
        "degraded"
    };
    Json(HealthResponse {
        status: status.to_owned(),
        database,
        providers,
    })
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    database: String,
    providers: std::collections::BTreeMap<String, ProviderHealth>,
}

#[derive(Serialize)]
struct ProviderHealth {
    active_accounts: i64,
}

#[derive(Deserialize)]
struct WebhookQuery {
    secret: String,
}

include!("webhook.rs");

#[derive(Clone)]
struct PinnedAsyncRouteSelection {
    account: ProviderAccount,
    egress_proxy: Option<EgressProxy>,
    selection_reason: String,
}

async fn proxy(
    State(state): State<GatewayState>,
    method: Method,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Result<Response, GatewayError> {
    let (provider, rest) = parse_provider_path(uri.path())?;
    proxy_request(state, provider, rest, method, headers, uri, body).await
}

fn parse_provider_path(path: &str) -> Result<(String, String), GatewayError> {
    let trimmed = path.trim_matches('/');
    let Some((provider, rest)) = trimmed
        .split_once('/')
        .map(|(provider, rest)| (provider, rest.to_owned()))
        .or_else(|| (!trimmed.is_empty()).then(|| (trimmed, String::new())))
    else {
        return Err(GatewayError::Provider(ProviderError::InvalidRoute(
            "missing provider route".to_owned(),
        )));
    };
    Ok((provider.to_owned(), rest))
}

async fn proxy_request(
    state: GatewayState,
    provider: String,
    rest: String,
    method: Method,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Result<Response, GatewayError> {
    let provider_id = ProviderId::from_str(provider.as_str())?;
    let extracted = extract_platform_credential(provider_id, headers, body)?;
    let platform_api_key = authorize(&extracted.token, &state.storage).await?;
    let headers = extracted.headers;
    let body = extracted.body;
    let start = Instant::now();
    let adapter = state
        .provider_registry
        .resolve(provider_id)
        .ok_or_else(|| GatewayError::ProviderUnavailable(provider_id))?;
    let route = adapter.parse_route(rest.as_str(), uri.query())?;
    let request_envelope = RequestEnvelope {
        request_id: Uuid::now_v7(),
        method: method.as_str().to_owned(),
        rest_path: rest,
        query: uri.query().map(ToOwned::to_owned),
        headers: flatten_headers(&headers),
        body: body.to_vec(),
        received_at: time::OffsetDateTime::now_utc(),
    };
    let pinned_async_route =
        resolve_pinned_async_route(&state, provider_id, &method, route.upstream_path.as_str())
            .await?;
    let route_incompatible_account_ids = if pinned_async_route.is_some() {
        Vec::new()
    } else {
        route_incompatible_account_ids(&state, adapter.as_ref(), provider_id, &route).await?
    };

    let max_retries = state.scheduler.config().max_retries;
    let using_pinned_async_route = pinned_async_route.is_some();
    let mut excluded_account_ids = route_incompatible_account_ids;
    let mut excluded_routes: Vec<RouteRetryExclusion> = Vec::new();
    let mut last_error: Option<GatewayError> = None;
    let mut last_retryable_response: Option<DeferredRetryableResponse> = None;

    for attempt in 0..=max_retries {
        let (selection_reason, account, egress_proxy) =
            if let Some(pinned_route) = &pinned_async_route {
                (
                    pinned_route.selection_reason.clone(),
                    pinned_route.account.clone(),
                    pinned_route.egress_proxy.clone(),
                )
            } else {
                let excluded_account_refs = excluded_account_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>();
                let excluded_route_refs = excluded_routes
                    .iter()
                    .map(|route| SchedulerRouteExclusion {
                        account_id: route.account_id.as_str(),
                        proxy_id: route.proxy_id.as_deref(),
                    })
                    .collect::<Vec<_>>();
                let selection = match state
                    .scheduler
                    .select_route_excluding(
                        &state.storage,
                        provider_id,
                        SchedulerExclusions {
                            account_ids: &excluded_account_refs,
                            routes: &excluded_route_refs,
                        },
                    )
                    .await
                {
                    Ok(selection) => selection,
                    Err(err) => {
                        if attempt > 0 {
                            if let Some(pending) = last_retryable_response.take() {
                                let DeferredRetryableResponse {
                                    account,
                                    egress_proxy,
                                    upstream,
                                    response_class,
                                    latency_ms,
                                    selection_reason,
                                    plan_url,
                                    route_upstream_path,
                                    webhook_secret,
                                    attempts,
                                } = pending;
                                return finalize_upstream_response(
                                    &state,
                                    FinalizeUpstreamResponseArgs {
                                        provider: provider.as_str(),
                                        provider_id,
                                        upstream_path: &route_upstream_path,
                                        request_envelope: &request_envelope,
                                        plan_url: &plan_url,
                                        account: &account,
                                        egress_proxy: egress_proxy.as_ref(),
                                        response_class,
                                        latency_ms,
                                        selection_reason: &selection_reason,
                                        attempts,
                                        platform_api_key: &platform_api_key,
                                        webhook_secret,
                                        outcome_recorded: true,
                                    },
                                    upstream,
                                )
                                .await;
                            }
                            break;
                        }
                        return Err(err.into());
                    }
                };
                (
                    selection.selection_reason,
                    selection.account,
                    selection.egress_proxy,
                )
            };
        let egress_mode = if egress_proxy.is_some() {
            "proxy"
        } else {
            "direct"
        };
        let egress_target = egress_proxy
            .as_ref()
            .map(|value| summarize_proxy_url(&value.proxy_url))
            .unwrap_or_else(|| "direct".to_owned());
        if let Err(storage_error) = state
            .storage
            .record_provider_account_selection(&account.id)
            .await
        {
            error!(
                provider = %provider_id,
                provider_account_id = %account.id,
                egress_mode,
                egress_target = %egress_target,
                error = %storage_error,
                "failed to record provider account selection"
            );
        }
        if let Some(egress_proxy) = &egress_proxy
            && let Err(storage_error) = state
                .storage
                .record_egress_proxy_selection(&egress_proxy.id)
                .await
        {
            error!(
                provider = %provider_id,
                provider_account_id = %account.id,
                egress_proxy_id = %egress_proxy.id,
                egress_mode,
                egress_target = %egress_target,
                error = %storage_error,
                "failed to record egress proxy selection"
            );
        }
        let plan = adapter.build_upstream_request(&request_envelope, &route, &account)?;
        let client = state.client_pool.client_for(egress_proxy.as_ref())?;
        let mut forwarded_body = body.clone();
        let mut webhook_secret: Option<String> = None;
        if provider_id == ProviderId::Firecrawl
            && let Some(webhook_base_url) = &state.webhook_base_url
        {
            let normalized = route.upstream_path.trim_matches('/');
            if matches!(normalized, "v2/crawl" | "v2/batch/scrape") {
                let secret = Uuid::now_v7().to_string();
                let webhook_url = format!(
                    "{}/webhooks/firecrawl?secret={}",
                    webhook_base_url.trim_end_matches('/'),
                    secret
                );
                if let Ok(mut json) =
                    serde_json::from_slice::<serde_json::Value>(forwarded_body.as_ref())
                {
                    if let Some(obj) = json.as_object_mut() {
                        obj.insert("webhook".to_owned(), serde_json::Value::String(webhook_url));
                    }
                    if let Ok(bytes) = serde_json::to_vec(&json) {
                        forwarded_body = Bytes::from(bytes);
                        webhook_secret = Some(secret);
                    }
                }
            }
        }
        let upstream = match forward_request(&client, &method, &headers, forwarded_body, &plan)
            .await
        {
            Ok(upstream) => upstream,
            Err(error) => {
                let error_message = error.to_string();
                if let Err(storage_error) = state
                    .storage
                    .record_provider_account_failure(&account.id, &error_message)
                    .await
                {
                    error!(
                        provider = %provider_id,
                        provider_account_id = %account.id,
                        egress_mode,
                        egress_target = %egress_target,
                        error = %storage_error,
                        "failed to record provider account transport error"
                    );
                }
                if let Some(egress_proxy) = &egress_proxy
                    && let Err(storage_error) = state
                        .storage
                        .record_egress_proxy_cooldown(
                            &egress_proxy.id,
                            calculate_cooldown_seconds(egress_proxy.consecutive_failures),
                            &error_message,
                        )
                        .await
                {
                    error!(
                        provider = %provider_id,
                        provider_account_id = %account.id,
                        egress_proxy_id = %egress_proxy.id,
                        egress_mode,
                        egress_target = %egress_target,
                        error = %storage_error,
                        "failed to record egress proxy transport error"
                    );
                }
                if attempt < max_retries {
                    info!(
                        provider = %provider_id,
                        provider_account_id = %account.id,
                        attempt,
                        pinned_async_route = using_pinned_async_route,
                        error = %error_message,
                        "{}",
                        if using_pinned_async_route {
                            "transport error, retrying tracked async route"
                        } else {
                            "transport error, retrying with different route"
                        }
                    );
                    if !using_pinned_async_route {
                        excluded_routes.push(RouteRetryExclusion {
                            account_id: account.id,
                            proxy_id: egress_proxy.as_ref().map(|value| value.id.clone()),
                        });
                    }
                    last_error = Some(error);
                    continue;
                }
                let latency_ms = i64::try_from(start.elapsed().as_millis()).ok();
                let request_log = build_request_log(
                    &state.request_log_capture,
                    BuildRequestLogArgs {
                        request_id: request_envelope.request_id,
                        tenant_id: Some((*state.default_tenant_id).clone()),
                        platform_api_key_id: Some(platform_api_key.id.clone()),
                        provider: provider_id,
                        provider_account_id: Some(account.id.clone()),
                        egress_proxy_id: egress_proxy.as_ref().map(|value| value.id.clone()),
                        method: request_envelope.method.clone(),
                        route: format_route(provider.as_str(), request_envelope.rest_path.as_str()),
                        upstream_url: plan.url.clone(),
                        status_code: None,
                        latency_ms,
                        failure_kind: Some(classify_gateway_failure(&error).to_owned()),
                        failure_message: Some(error_message),
                        request_headers: request_envelope.headers.clone(),
                        response_headers: HeaderValues::new(),
                        request_body_bytes: request_envelope.body.as_slice(),
                        response_body_bytes: None,
                    },
                );
                enqueue_persistence(
                    &state,
                    PersistenceBundle {
                        request_log: request_log.clone(),
                        async_job: None,
                    },
                    PersistenceContext {
                        request_id: request_log.id,
                        provider_id,
                        provider_account_id: Some(account.id.as_str()),
                        egress_proxy_id: egress_proxy.as_ref().map(|value| value.id.as_str()),
                        egress_mode,
                        egress_target: egress_target.as_str(),
                    },
                )
                .await;
                return Err(error);
            }
        };
        let latency_ms = i64::try_from(start.elapsed().as_millis()).ok();
        let response_class = adapter.classify_response(upstream.status.as_u16());

        if response_class.retryable && attempt < max_retries {
            info!(
                provider = %provider_id,
                provider_account_id = %account.id,
                status = upstream.status.as_u16(),
                attempt,
                pinned_async_route = using_pinned_async_route,
                "{}",
                if using_pinned_async_route {
                    "retryable response, retrying tracked async route"
                } else {
                    "retryable response, trying next account"
                }
            );
            if let Err(storage_error) = record_provider_account_outcome(
                &state,
                &account.id,
                account.consecutive_failures,
                upstream.status,
                response_class.disposition,
            )
            .await
            {
                error!(
                    provider = %provider_id,
                    provider_account_id = %account.id,
                    error = %storage_error,
                    "failed to update provider account outcome before retry"
                );
            }
            if let Some(egress_proxy) = &egress_proxy
                && let Err(storage_error) = state
                    .storage
                    .record_egress_proxy_success(&egress_proxy.id)
                    .await
            {
                error!(
                    provider = %provider_id,
                    provider_account_id = %account.id,
                    egress_proxy_id = %egress_proxy.id,
                    error = %storage_error,
                    "failed to update egress proxy outcome before retry"
                );
            }
            let excluded_account_id = account.id.clone();
            last_retryable_response = Some(DeferredRetryableResponse {
                account,
                egress_proxy,
                upstream,
                response_class,
                latency_ms,
                selection_reason,
                plan_url: plan.url.clone(),
                route_upstream_path: route.upstream_path.clone(),
                webhook_secret,
                attempts: attempt + 1,
            });
            if !using_pinned_async_route {
                excluded_account_ids.push(excluded_account_id);
            }
            continue;
        }

        return finalize_upstream_response(
            &state,
            FinalizeUpstreamResponseArgs {
                provider: provider.as_str(),
                provider_id,
                upstream_path: &route.upstream_path,
                request_envelope: &request_envelope,
                plan_url: &plan.url,
                account: &account,
                egress_proxy: egress_proxy.as_ref(),
                response_class,
                latency_ms,
                selection_reason: &selection_reason,
                attempts: attempt + 1,
                platform_api_key: &platform_api_key,
                webhook_secret,
                outcome_recorded: false,
            },
            upstream,
        )
        .await;
    }

    Err(last_error.unwrap_or_else(|| GatewayError::ProviderUnavailable(provider_id)))
}

async fn route_incompatible_account_ids(
    state: &GatewayState,
    adapter: &dyn ProviderAdapter,
    provider_id: ProviderId,
    route: &wdapm_core::ProviderRoute,
) -> Result<Vec<String>, GatewayError> {
    let accounts = state
        .storage
        .list_routable_provider_accounts(provider_id)
        .await?;
    Ok(route_incompatible_account_ids_from_accounts(
        accounts, adapter, route,
    ))
}

fn route_incompatible_account_ids_from_accounts(
    accounts: Vec<ProviderAccount>,
    adapter: &dyn ProviderAdapter,
    route: &wdapm_core::ProviderRoute,
) -> Vec<String> {
    accounts
        .into_iter()
        .filter(|account| !adapter.supports_account_for_route(route, account))
        .map(|account| account.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wdapm_core::ProviderAccountStatus;

    #[test]
    fn route_incompatible_account_ids_excludes_unsupported_accounts() {
        let adapter = RejectKeylessAdapter;
        let route = wdapm_core::ProviderRoute {
            base_url_override: None,
            upstream_path: "/test".to_owned(),
            query: None,
        };
        let account_ids = route_incompatible_account_ids_from_accounts(
            vec![
                provider_account("blank", ""),
                provider_account("keyed", "secret"),
            ],
            &adapter,
            &route,
        );

        assert_eq!(account_ids, vec!["blank"]);
    }

    struct RejectKeylessAdapter;

    impl ProviderAdapter for RejectKeylessAdapter {
        fn provider_id(&self) -> ProviderId {
            ProviderId::Jina
        }

        fn parse_route(
            &self,
            _rest_path: &str,
            _query: Option<&str>,
        ) -> Result<wdapm_core::ProviderRoute, ProviderError> {
            unreachable!()
        }

        fn build_upstream_request(
            &self,
            _request: &RequestEnvelope,
            _route: &wdapm_core::ProviderRoute,
            _account: &ProviderAccount,
        ) -> Result<UpstreamRequestPlan, ProviderError> {
            unreachable!()
        }

        fn supports_account_for_route(
            &self,
            _route: &wdapm_core::ProviderRoute,
            account: &ProviderAccount,
        ) -> bool {
            !account.api_key.is_empty()
        }

        fn classify_response(&self, _status: u16) -> ProviderResponseClass {
            ProviderResponseClass::passthrough()
        }
    }

    fn provider_account(id: &str, api_key: &str) -> ProviderAccount {
        ProviderAccount {
            id: id.to_owned(),
            provider: ProviderId::Jina,
            name: id.to_owned(),
            api_key: api_key.to_owned(),
            base_url: None,
            reader_base_url: None,
            search_base_url: None,
            enabled: true,
            status: ProviderAccountStatus::Active,
            last_error: None,
            cooldown_until: None,
            last_used_at: None,
            consecutive_failures: 0,
            last_status_code: None,
            weight: 100,
            last_failure_at: None,
        }
    }
}

async fn resolve_pinned_async_route(
    state: &GatewayState,
    provider_id: ProviderId,
    method: &Method,
    upstream_path: &str,
) -> Result<Option<PinnedAsyncRouteSelection>, GatewayError> {
    if provider_id != ProviderId::Firecrawl || *method != Method::GET {
        return Ok(None);
    }
    let Some((route, upstream_job_id)) = parse_firecrawl_async_status_path(upstream_path) else {
        return Ok(None);
    };
    let Some(job) = state
        .storage
        .find_provider_async_job_by_upstream_id(provider_id, upstream_job_id)
        .await
        .map_err(GatewayError::WebhookStorage)?
    else {
        info!(
            provider = %provider_id,
            route,
            upstream_job_id,
            "firecrawl async status request not pinned because no tracked job was found"
        );
        return Ok(None);
    };
    let Some(account_id) = job.provider_account_id.as_deref() else {
        return Err(GatewayError::Provider(ProviderError::InvalidRoute(
            format!(
                "tracked async job `{}` is missing provider account binding",
                job.id
            ),
        )));
    };
    let Some(account) = state
        .storage
        .find_provider_account(account_id)
        .await
        .map_err(GatewayError::WebhookStorage)?
    else {
        return Err(GatewayError::Provider(ProviderError::InvalidRoute(
            format!(
                "tracked async job `{}` provider account `{account_id}` was not found",
                job.id
            ),
        )));
    };
    let egress_proxy = match job.egress_proxy_id.as_deref() {
        Some(proxy_id) => {
            let proxy = state
                .storage
                .find_egress_proxy(proxy_id)
                .await
                .map_err(GatewayError::WebhookStorage)?;
            if proxy.is_none() {
                return Err(GatewayError::Provider(ProviderError::InvalidRoute(
                    format!(
                        "tracked async job `{}` egress proxy `{proxy_id}` was not found",
                        job.id
                    ),
                )));
            }
            proxy
        }
        None => None,
    };
    let selection_reason = match &egress_proxy {
        Some(proxy) => format!(
            "async_job:{} upstream_job:{} account:{} egress:proxy id:{} target:{}",
            job.id,
            job.upstream_job_id,
            account.id,
            proxy.id,
            summarize_proxy_url(&proxy.proxy_url)
        ),
        None => format!(
            "async_job:{} upstream_job:{} account:{} egress:direct reason:tracked_async_job",
            job.id, job.upstream_job_id, account.id
        ),
    };
    info!(
        async_job_id = job.id.as_str(),
        upstream_job_id = job.upstream_job_id.as_str(),
        provider_account_id = account.id.as_str(),
        egress_proxy_id = egress_proxy.as_ref().map(|value| value.id.as_str()),
        selection_reason = selection_reason.as_str(),
        "firecrawl async status request pinned to tracked route"
    );
    Ok(Some(PinnedAsyncRouteSelection {
        account,
        egress_proxy,
        selection_reason,
    }))
}

fn parse_firecrawl_async_status_path(upstream_path: &str) -> Option<(&str, &str)> {
    let normalized = upstream_path.trim_matches('/');
    normalized
        .strip_prefix("v2/crawl/")
        .map(|job_id| ("v2/crawl", job_id))
        .or_else(|| {
            normalized
                .strip_prefix("v2/batch/scrape/")
                .map(|job_id| ("v2/batch/scrape", job_id))
        })
        .filter(|(_, job_id)| !job_id.is_empty() && !job_id.contains('/'))
}

async fn forward_request(
    client: &Client,
    method: &Method,
    headers: &HeaderMap,
    body: Bytes,
    plan: &UpstreamRequestPlan,
) -> Result<UpstreamResponseData, GatewayError> {
    let mut builder = client.request(method.clone(), &plan.url);

    for (name, value) in headers {
        if should_forward_request_header(name) {
            builder = builder.header(name, value);
        }
    }

    builder = match &plan.auth {
        ProviderAuth::Bearer(value) => builder.bearer_auth(value),
        ProviderAuth::Header { name, value } => builder.header(name.as_str(), value.as_str()),
        ProviderAuth::None => builder,
    };

    let body = plan
        .body_override
        .as_deref()
        .map(Bytes::copy_from_slice)
        .unwrap_or(body);
    if !body.is_empty() {
        builder = builder.body(body);
    }

    let upstream = builder.send().await?;
    let status = upstream.status();
    let version = upstream.version();
    let upstream_headers = upstream.headers().clone();
    let upstream_body = upstream.bytes().await?;

    Ok(UpstreamResponseData {
        status,
        version,
        headers: upstream_headers,
        body: upstream_body,
    })
}

fn build_response(
    upstream: UpstreamResponseData,
    provider_id: ProviderId,
) -> Result<Response, GatewayError> {
    let mut response_builder = Response::builder()
        .status(upstream.status)
        .version(upstream.version)
        .header("x-wdapm-provider", provider_id.as_str());

    for (name, value) in &upstream.headers {
        if should_forward_response_header(name) {
            response_builder = response_builder.header(name, value);
        }
    }

    response_builder
        .body(Body::from(upstream.body))
        .map_err(GatewayError::from)
}

include!("auth.rs");

async fn record_provider_account_outcome(
    state: &GatewayState,
    account_id: &str,
    consecutive_failures: i64,
    status: StatusCode,
    disposition: ResponseDisposition,
) -> Result<(), wdapm_storage::StorageError> {
    match disposition {
        ResponseDisposition::Cooldown => {
            let cooldown_secs = calculate_cooldown_seconds(consecutive_failures);
            state
                .storage
                .record_provider_account_cooldown(
                    account_id,
                    cooldown_secs,
                    Some(i64::from(status.as_u16())),
                    Some("upstream rate limited"),
                )
                .await
        }
        ResponseDisposition::DisableAccount => {
            state
                .storage
                .record_provider_account_disabled(
                    account_id,
                    Some(i64::from(status.as_u16())),
                    Some("upstream authentication failed"),
                )
                .await
        }
        ResponseDisposition::Passthrough if status.is_server_error() => {
            let reason = format!("upstream status {}", status.as_u16());
            state
                .storage
                .record_provider_account_failure(account_id, &reason)
                .await
        }
        ResponseDisposition::Passthrough => {
            state
                .storage
                .record_provider_account_success(account_id, Some(i64::from(status.as_u16())))
                .await
        }
    }
}

#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("missing platform authorization header")]
    MissingAuthorization,
    #[error("platform authorization header is invalid")]
    InvalidAuthorization,
    #[error("conflicting platform credentials")]
    AmbiguousAuthorization,
    #[error("platform api key is invalid")]
    InvalidPlatformKey,
    #[error("api key quota exceeded")]
    QuotaExceeded,
    #[error("provider `{0}` is unavailable")]
    ProviderUnavailable(ProviderId),
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Scheduler(#[from] wdapm_scheduler::SchedulerError),
    #[error(transparent)]
    Storage(#[from] wdapm_storage::StorageError),
    #[error("gateway client pool is poisoned")]
    ClientPoolPoisoned,
    #[error(transparent)]
    WebhookStorage(wdapm_storage::StorageError),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    ResponseBuild(#[from] axum::http::Error),
}

impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::MissingAuthorization | Self::InvalidAuthorization | Self::InvalidPlatformKey => {
                (StatusCode::UNAUTHORIZED, self.to_string())
            }
            Self::AmbiguousAuthorization => (StatusCode::BAD_REQUEST, self.to_string()),
            Self::QuotaExceeded => (StatusCode::TOO_MANY_REQUESTS, self.to_string()),
            Self::ProviderUnavailable(_) => (StatusCode::SERVICE_UNAVAILABLE, self.to_string()),
            Self::Scheduler(_) => {
                error!(error = %self, "gateway scheduler error");
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "service temporarily unavailable".to_owned(),
                )
            }
            Self::Provider(ref err) => {
                error!(%err, "gateway provider error");
                (StatusCode::BAD_REQUEST, "provider request error".to_owned())
            }
            Self::ClientPoolPoisoned
            | Self::Http(_)
            | Self::ResponseBuild(_)
            | Self::Storage(_)
            | Self::WebhookStorage(_) => {
                error!(error = %self, "gateway internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".to_owned(),
                )
            }
        };

        (status, message).into_response()
    }
}

struct UpstreamResponseData {
    status: StatusCode,
    version: Version,
    headers: HeaderMap,
    body: Bytes,
}

struct DeferredRetryableResponse {
    account: ProviderAccount,
    egress_proxy: Option<EgressProxy>,
    upstream: UpstreamResponseData,
    response_class: ProviderResponseClass,
    latency_ms: Option<i64>,
    selection_reason: String,
    plan_url: String,
    route_upstream_path: String,
    webhook_secret: Option<String>,
    attempts: u32,
}

struct RouteRetryExclusion {
    account_id: String,
    proxy_id: Option<String>,
}

struct FinalizeUpstreamResponseArgs<'a> {
    provider: &'a str,
    provider_id: ProviderId,
    upstream_path: &'a str,
    request_envelope: &'a RequestEnvelope,
    plan_url: &'a str,
    account: &'a ProviderAccount,
    egress_proxy: Option<&'a EgressProxy>,
    response_class: ProviderResponseClass,
    latency_ms: Option<i64>,
    selection_reason: &'a str,
    attempts: u32,
    platform_api_key: &'a PlatformApiKeyRecord,
    webhook_secret: Option<String>,
    outcome_recorded: bool,
}

include!("persistence.rs");
