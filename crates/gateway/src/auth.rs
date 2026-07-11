struct ExtractedPlatformCredential {
    token: String,
    headers: HeaderMap,
    body: Bytes,
}

fn extract_platform_credential(
    provider: ProviderId,
    mut headers: HeaderMap,
    body: Bytes,
) -> Result<ExtractedPlatformCredential, GatewayError> {
    let bearer = headers
        .remove(AUTHORIZATION)
        .map(|value| {
            let raw = value
                .to_str()
                .map_err(|_| GatewayError::InvalidAuthorization)?;
            raw.strip_prefix("Bearer ")
                .or_else(|| raw.strip_prefix("bearer "))
                .map(ToOwned::to_owned)
                .ok_or(GatewayError::InvalidAuthorization)
        })
        .transpose()?;
    let provider_header = if provider == ProviderId::Exa {
        headers
            .remove("x-api-key")
            .map(|value| {
                value
                    .to_str()
                    .map(str::to_owned)
                    .map_err(|_| GatewayError::InvalidAuthorization)
            })
            .transpose()?
    } else {
        None
    };
    let (body_token, body) = if provider == ProviderId::Tavily && !body.is_empty() {
        sanitize_tavily_body(body)?
    } else {
        (None, body)
    };
    let candidates = [bearer, provider_header, body_token]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let Some(token) = candidates.first().cloned() else {
        return Err(GatewayError::MissingAuthorization);
    };
    if candidates.iter().any(|candidate| candidate != &token) {
        return Err(GatewayError::AmbiguousAuthorization);
    }
    Ok(ExtractedPlatformCredential {
        token,
        headers,
        body,
    })
}

fn sanitize_tavily_body(body: Bytes) -> Result<(Option<String>, Bytes), GatewayError> {
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return Ok((None, body));
    };
    let Some(object) = value.as_object_mut() else {
        return Ok((None, body));
    };
    let Some(api_key) = object.get("api_key") else {
        return Ok((None, body));
    };
    let api_key = api_key
        .as_str()
        .ok_or(GatewayError::InvalidAuthorization)?
        .to_owned();
    object.insert("api_key".to_owned(), serde_json::Value::String(String::new()));
    let sanitized = serde_json::to_vec(&value)
        .map(Bytes::from)
        .map_err(|_| GatewayError::InvalidAuthorization)?;
    Ok((Some(api_key), sanitized))
}

async fn authorize(
    token: &str,
    storage: &StorageService,
) -> Result<PlatformApiKeyRecord, GatewayError> {
    let key_hash = hash_token(token);
    let key_record = storage
        .find_platform_api_key_by_hash(&key_hash)
        .await
        .map_err(GatewayError::WebhookStorage)?;

    let Some(key_record) = key_record else {
        return Err(GatewayError::InvalidPlatformKey);
    };

    if !storage
        .increment_platform_api_key_counter(&key_record.id)
        .await
        .map_err(GatewayError::WebhookStorage)?
    {
        return Err(GatewayError::QuotaExceeded);
    }

    Ok(key_record)
}

#[cfg(test)]
mod auth_tests {
    use super::*;

    #[test]
    fn extracts_exa_x_api_key_and_removes_header() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", "wdapm_test".parse().unwrap());

        let extracted = extract_platform_credential(ProviderId::Exa, headers, Bytes::new()).unwrap();

        assert_eq!(extracted.token, "wdapm_test");
        assert!(!extracted.headers.contains_key("x-api-key"));
    }

    #[test]
    fn extracts_tavily_body_key_and_sanitizes_body() {
        let body = Bytes::from_static(br#"{"query":"test","api_key":"wdapm_test"}"#);

        let extracted = extract_platform_credential(ProviderId::Tavily, HeaderMap::new(), body).unwrap();

        assert_eq!(extracted.token, "wdapm_test");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&extracted.body).unwrap()["api_key"],
            ""
        );
    }

    #[test]
    fn rejects_conflicting_credentials() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, "Bearer first".parse().unwrap());
        headers.insert("x-api-key", "second".parse().unwrap());

        let result = extract_platform_credential(ProviderId::Exa, headers, Bytes::new());

        assert!(matches!(result, Err(GatewayError::AmbiguousAuthorization)));
    }
}

fn flatten_headers(headers: &HeaderMap) -> HeaderValues {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_owned(), value.to_owned()))
        })
        .collect()
}

fn should_forward_request_header(name: &HeaderName) -> bool {
    name != AUTHORIZATION
        && name != CONNECTION
        && name != CONTENT_LENGTH
        && name != HOST
        && name != TRANSFER_ENCODING
}

fn should_forward_response_header(name: &HeaderName) -> bool {
    name != CONNECTION && name != CONTENT_LENGTH && name != TRANSFER_ENCODING
}

fn format_route(provider: &str, rest: &str) -> String {
    let trimmed = rest.trim_matches('/');
    if trimmed.is_empty() {
        format!("/{provider}")
    } else {
        format!("/{provider}/{trimmed}")
    }
}
