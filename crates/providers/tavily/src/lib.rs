use std::sync::Arc;

use serde_json::Value;

use wdapm_core::{
    ProviderAccount, ProviderAdapter, ProviderAuth, ProviderError, ProviderId,
    ProviderResponseClass, ProviderRoute, RequestEnvelope, UpstreamRequestPlan, join_url,
    normalize_rest_path,
};

pub fn adapter() -> Arc<dyn ProviderAdapter> {
    Arc::new(TavilyAdapter)
}

struct TavilyAdapter;

impl ProviderAdapter for TavilyAdapter {
    fn provider_id(&self) -> ProviderId {
        ProviderId::Tavily
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
        request: &RequestEnvelope,
        route: &ProviderRoute,
        account: &ProviderAccount,
    ) -> Result<UpstreamRequestPlan, ProviderError> {
        let body_override = replace_body_api_key(&request.body, &account.api_key)?;
        Ok(UpstreamRequestPlan {
            provider: ProviderId::Tavily,
            url: join_url(
                account.base_url(),
                &route.upstream_path,
                route.query.as_deref(),
            ),
            auth: ProviderAuth::Bearer(account.api_key.clone()),
            body_override,
        })
    }

    fn classify_response(&self, status: u16) -> ProviderResponseClass {
        match status {
            401 | 403 => ProviderResponseClass::disable_account(),
            429 => ProviderResponseClass::cooldown(),
            500..=599 => ProviderResponseClass::retryable(),
            _ => ProviderResponseClass::passthrough(),
        }
    }
}

fn replace_body_api_key(body: &[u8], api_key: &str) -> Result<Option<Vec<u8>>, ProviderError> {
    if body.is_empty() {
        return Ok(None);
    }
    let Ok(mut value) = serde_json::from_slice::<Value>(body) else {
        return Ok(None);
    };
    let Some(object) = value.as_object_mut() else {
        return Ok(None);
    };
    if !object.contains_key("api_key") {
        return Ok(None);
    }
    object.insert("api_key".to_owned(), Value::String(api_key.to_owned()));
    serde_json::to_vec(&value)
        .map(Some)
        .map_err(|error| ProviderError::InvalidRoute(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::OffsetDateTime;
    use uuid::Uuid;
    use wdapm_core::{HeaderValues, ProviderAccountStatus};

    #[test]
    fn replaces_sanitized_body_key_with_selected_account_key() {
        let adapter = TavilyAdapter;
        let route = adapter.parse_route("search", None).unwrap();
        let request = RequestEnvelope {
            request_id: Uuid::nil(),
            method: "POST".to_owned(),
            rest_path: "search".to_owned(),
            query: None,
            headers: HeaderValues::new(),
            body: br#"{"query":"test","api_key":""}"#.to_vec(),
            received_at: OffsetDateTime::UNIX_EPOCH,
        };
        let account = ProviderAccount {
            id: "tavily-test".to_owned(),
            provider: ProviderId::Tavily,
            name: "Tavily Test".to_owned(),
            api_key: "tvly-upstream".to_owned(),
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
        };

        let plan = adapter
            .build_upstream_request(&request, &route, &account)
            .unwrap();
        let body = serde_json::from_slice::<Value>(&plan.body_override.unwrap()).unwrap();

        assert_eq!(body["api_key"], "tvly-upstream");
    }
}
