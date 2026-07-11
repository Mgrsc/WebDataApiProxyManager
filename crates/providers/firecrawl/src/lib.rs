use std::sync::Arc;

use serde_json::Value;
use wdapm_core::{
    ProviderAccount, ProviderAdapter, ProviderAsyncJobState, ProviderAuth, ProviderError,
    ProviderId, ProviderResponseClass, ProviderRoute, RequestEnvelope, UpstreamRequestPlan,
    join_url, normalize_rest_path,
};

const KEYLESS_SEARCH_ROUTE: &str = "search";
const KEYLESS_SCRAPE_ROUTE: &str = "scrape";
const KEYLESS_INTERACT_ROUTE: &str = "interact";

pub fn adapter() -> Arc<dyn ProviderAdapter> {
    Arc::new(FirecrawlAdapter)
}

pub struct FirecrawlAsyncJobDiscovery {
    pub route: String,
    pub upstream_job_id: String,
    pub status_path: String,
    pub metadata: Value,
}

pub struct FirecrawlAsyncJobStatus {
    pub state: ProviderAsyncJobState,
    pub metadata: Value,
}

struct FirecrawlAdapter;

impl ProviderAdapter for FirecrawlAdapter {
    fn provider_id(&self) -> ProviderId {
        ProviderId::Firecrawl
    }

    fn parse_route(
        &self,
        rest_path: &str,
        query: Option<&str>,
    ) -> Result<ProviderRoute, ProviderError> {
        Ok(ProviderRoute {
            base_url_override: None,
            upstream_path: normalize_rest_path(rest_path)?,
            query: query.map(ToOwned::to_owned),
        })
    }

    fn build_upstream_request(
        &self,
        _request: &RequestEnvelope,
        route: &ProviderRoute,
        account: &ProviderAccount,
    ) -> Result<UpstreamRequestPlan, ProviderError> {
        Ok(UpstreamRequestPlan {
            provider: ProviderId::Firecrawl,
            url: join_url(
                account.base_url(),
                &route.upstream_path,
                route.query.as_deref(),
            ),
            auth: firecrawl_auth(account),
            body_override: None,
        })
    }

    fn supports_account_for_route(&self, route: &ProviderRoute, account: &ProviderAccount) -> bool {
        !account.api_key.trim().is_empty() || is_keyless_firecrawl_route(&route.upstream_path)
    }

    fn classify_response(&self, status: u16) -> ProviderResponseClass {
        match status {
            401 | 403 => ProviderResponseClass::disable_account(),
            402 | 429 => ProviderResponseClass::cooldown(),
            500..=599 => ProviderResponseClass::retryable(),
            _ => ProviderResponseClass::passthrough(),
        }
    }
}

fn firecrawl_auth(account: &ProviderAccount) -> ProviderAuth {
    if account.api_key.trim().is_empty() {
        ProviderAuth::None
    } else {
        ProviderAuth::Bearer(account.api_key.clone())
    }
}

fn is_keyless_firecrawl_route(upstream_path: &str) -> bool {
    let normalized = upstream_path.trim_matches('/');
    let normalized = normalized.strip_prefix("v2/").unwrap_or(normalized);
    normalized == KEYLESS_SEARCH_ROUTE
        || normalized == KEYLESS_SCRAPE_ROUTE
        || normalized == KEYLESS_INTERACT_ROUTE
        || is_keyless_interact_route(normalized)
}

fn is_keyless_interact_route(normalized_path: &str) -> bool {
    let mut segments = normalized_path.split('/');
    matches!(segments.next(), Some(KEYLESS_SCRAPE_ROUTE))
        && matches!(segments.next(), Some(value) if !value.is_empty())
        && matches!(segments.next(), Some(KEYLESS_INTERACT_ROUTE))
        && segments.next().is_none()
}

pub fn detect_async_job(
    route: &str,
    status: u16,
    body: &[u8],
) -> Result<Option<FirecrawlAsyncJobDiscovery>, ProviderError> {
    if !(200..=299).contains(&status) {
        return Ok(None);
    }

    let normalized_route = route.trim_matches('/');
    let status_path_template = match normalized_route {
        "v2/crawl" => "/v2/crawl/{id}",
        "v2/batch/scrape" => "/v2/batch/scrape/{id}",
        _ => return Ok(None),
    };
    let json = parse_json_body(body)?;
    if !json
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(None);
    }
    let Some(upstream_job_id) = json.get("id").and_then(Value::as_str) else {
        return Ok(None);
    };
    let status_path = status_path_template.replace("{id}", upstream_job_id);

    Ok(Some(FirecrawlAsyncJobDiscovery {
        route: format!("/{normalized_route}"),
        upstream_job_id: upstream_job_id.to_owned(),
        status_path: status_path.clone(),
        metadata: serde_json::json!({
            "create_route": format!("/{normalized_route}"),
            "status_path": status_path,
            "submission": json,
        }),
    }))
}

pub fn parse_async_job_status(body: &[u8]) -> Result<FirecrawlAsyncJobStatus, ProviderError> {
    let json = parse_json_body(body)?;
    let raw_status = json
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("pending");
    let state = match raw_status {
        "completed" => ProviderAsyncJobState::Completed,
        "failed" => ProviderAsyncJobState::Failed,
        "cancelled" | "canceled" => ProviderAsyncJobState::Cancelled,
        "scraping" | "processing" | "queued" | "pending" => ProviderAsyncJobState::Running,
        _ => ProviderAsyncJobState::Pending,
    };

    Ok(FirecrawlAsyncJobStatus {
        state,
        metadata: serde_json::json!({
            "status": raw_status,
            "status_payload": json,
        }),
    })
}

pub fn parse_webhook_payload(body: &[u8]) -> Result<FirecrawlAsyncJobStatus, ProviderError> {
    let json = parse_json_body(body)?;
    let raw_status = json
        .get("status")
        .and_then(Value::as_str)
        .or_else(|| json.get("type").and_then(Value::as_str))
        .unwrap_or("pending");
    let state = match raw_status {
        "completed" | "crawl.completed" | "batch_scrape.completed" => {
            ProviderAsyncJobState::Completed
        }
        "failed" | "crawl.failed" | "batch_scrape.failed" => ProviderAsyncJobState::Failed,
        "cancelled" | "canceled" => ProviderAsyncJobState::Cancelled,
        _ => ProviderAsyncJobState::Running,
    };

    Ok(FirecrawlAsyncJobStatus {
        state,
        metadata: serde_json::json!({
            "source": "webhook",
            "webhook_payload": json,
        }),
    })
}

fn parse_json_body(body: &[u8]) -> Result<Value, ProviderError> {
    if body.is_empty() {
        return Ok(Value::Null);
    }

    serde_json::from_slice(body).map_err(|error| {
        ProviderError::InvalidRoute(format!("invalid firecrawl json payload: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wdapm_core::ProviderAccountStatus;

    #[test]
    fn keyless_account_supports_search_route_without_auth() {
        let adapter = FirecrawlAdapter;
        let route = adapter
            .parse_route("v2/search", None)
            .expect("search route should parse");
        let account = provider_account("");

        assert!(adapter.supports_account_for_route(&route, &account));
        assert!(matches!(firecrawl_auth(&account), ProviderAuth::None));
    }

    #[test]
    fn keyless_account_supports_scrape_interact_route() {
        let adapter = FirecrawlAdapter;
        let route = adapter
            .parse_route("v2/scrape/scrape-id/interact", None)
            .expect("interact route should parse");
        let account = provider_account("");

        assert!(adapter.supports_account_for_route(&route, &account));
    }

    #[test]
    fn keyless_account_does_not_support_crawl_route() {
        let adapter = FirecrawlAdapter;
        let route = adapter
            .parse_route("v2/crawl", None)
            .expect("crawl route should parse");
        let account = provider_account("");

        assert!(!adapter.supports_account_for_route(&route, &account));
    }

    #[test]
    fn keyed_account_supports_crawl_route() {
        let adapter = FirecrawlAdapter;
        let route = adapter
            .parse_route("v2/crawl", None)
            .expect("crawl route should parse");
        let account = provider_account("fc-test");

        assert!(adapter.supports_account_for_route(&route, &account));
    }

    fn provider_account(api_key: &str) -> ProviderAccount {
        ProviderAccount {
            id: "firecrawl-test".to_owned(),
            provider: ProviderId::Firecrawl,
            name: "Firecrawl Test".to_owned(),
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
