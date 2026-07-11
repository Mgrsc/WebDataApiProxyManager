use std::sync::Arc;

use wdapm_core::{
    ProviderAccount, ProviderAdapter, ProviderAuth, ProviderError, ProviderId,
    ProviderResponseClass, ProviderRoute, RequestEnvelope, UpstreamRequestPlan, join_url,
};

const JINA_READER_BASE_URL: &str = "https://r.jina.ai";
const JINA_SEARCH_BASE_URL: &str = "https://s.jina.ai";

pub fn adapter() -> Arc<dyn ProviderAdapter> {
    Arc::new(JinaAdapter)
}

struct JinaAdapter;

impl ProviderAdapter for JinaAdapter {
    fn provider_id(&self) -> ProviderId {
        ProviderId::Jina
    }

    fn parse_route(
        &self,
        rest_path: &str,
        query: Option<&str>,
    ) -> Result<ProviderRoute, ProviderError> {
        let trimmed = rest_path.trim_matches('/');
        let (base_url_override, upstream_path) = if let Some(target) = trimmed.strip_prefix("r/") {
            (
                JINA_READER_BASE_URL.to_owned(),
                format!("/{}", target.trim_start_matches('/')),
            )
        } else if let Some(target) = trimmed.strip_prefix("s/") {
            (
                JINA_SEARCH_BASE_URL.to_owned(),
                format!("/{}", target.trim_start_matches('/')),
            )
        } else {
            return Err(ProviderError::InvalidRoute(
                "jina routes must start with `r/` or `s/`".to_owned(),
            ));
        };

        if upstream_path == "/" {
            return Err(ProviderError::InvalidRoute(
                "jina target path cannot be empty".to_owned(),
            ));
        }

        Ok(ProviderRoute {
            base_url_override: Some(base_url_override),
            upstream_path,
            query: query.map(ToOwned::to_owned),
        })
    }

    fn build_upstream_request(
        &self,
        _request: &RequestEnvelope,
        route: &ProviderRoute,
        account: &ProviderAccount,
    ) -> Result<UpstreamRequestPlan, ProviderError> {
        let base_url = route
            .base_url_override
            .as_deref()
            .map(|route_base_url| match route_base_url {
                JINA_READER_BASE_URL => account
                    .reader_base_url
                    .as_deref()
                    .or(account.base_url.as_deref())
                    .unwrap_or(JINA_READER_BASE_URL),
                JINA_SEARCH_BASE_URL => account
                    .search_base_url
                    .as_deref()
                    .unwrap_or(JINA_SEARCH_BASE_URL),
                _ => route_base_url,
            })
            .unwrap_or(account.base_url());
        let auth = if account.api_key.is_empty() {
            ProviderAuth::None
        } else {
            ProviderAuth::Bearer(account.api_key.clone())
        };

        Ok(UpstreamRequestPlan {
            provider: ProviderId::Jina,
            url: join_url(base_url, &route.upstream_path, route.query.as_deref()),
            auth,
            body_override: None,
        })
    }

    fn supports_account_for_route(&self, route: &ProviderRoute, account: &ProviderAccount) -> bool {
        !account.api_key.trim().is_empty()
            || route.base_url_override.as_deref() == Some(JINA_READER_BASE_URL)
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

#[cfg(test)]
mod tests {
    use super::*;
    use wdapm_core::ProviderAccountStatus;

    #[test]
    fn keyless_account_supports_reader_route() {
        let adapter = JinaAdapter;
        let route = adapter
            .parse_route("r/https://example.com", None)
            .expect("reader route should parse");
        let account = provider_account("");

        assert!(adapter.supports_account_for_route(&route, &account));
    }

    #[test]
    fn keyless_account_does_not_support_search_route() {
        let adapter = JinaAdapter;
        let route = adapter
            .parse_route("s/search", Some("q=example"))
            .expect("search route should parse");
        let account = provider_account("");

        assert!(!adapter.supports_account_for_route(&route, &account));
    }

    #[test]
    fn keyed_account_supports_search_route() {
        let adapter = JinaAdapter;
        let route = adapter
            .parse_route("s/search", Some("q=example"))
            .expect("search route should parse");
        let account = provider_account("jina_test");

        assert!(adapter.supports_account_for_route(&route, &account));
    }

    #[test]
    fn uses_account_specific_reader_and_search_urls() {
        let adapter = JinaAdapter;
        let mut account = provider_account("jina_test");
        account.reader_base_url = Some("https://reader.example.com".to_owned());
        account.search_base_url = Some("https://search.example.com".to_owned());
        let request = RequestEnvelope {
            request_id: uuid::Uuid::nil(),
            method: "GET".to_owned(),
            rest_path: String::new(),
            query: None,
            headers: wdapm_core::HeaderValues::new(),
            body: Vec::new(),
            received_at: time::OffsetDateTime::UNIX_EPOCH,
        };

        let reader = adapter.parse_route("r/https://example.com", None).unwrap();
        let search = adapter.parse_route("s/search", Some("q=test")).unwrap();

        assert!(
            adapter
                .build_upstream_request(&request, &reader, &account)
                .unwrap()
                .url
                .starts_with("https://reader.example.com/")
        );
        assert!(
            adapter
                .build_upstream_request(&request, &search, &account)
                .unwrap()
                .url
                .starts_with("https://search.example.com/")
        );
    }

    fn provider_account(api_key: &str) -> ProviderAccount {
        ProviderAccount {
            id: "jina-test".to_owned(),
            provider: ProviderId::Jina,
            name: "Jina Test".to_owned(),
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
