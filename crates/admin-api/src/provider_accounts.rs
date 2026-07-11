async fn list_provider_accounts(
    State(state): State<AdminApiState>,
    headers: HeaderMap,
    Query(query): Query<ProviderAccountListQuery>,
) -> Result<Json<Vec<ProviderAccountSummary>>, AdminApiError> {
    authorize(&headers, &state.storage).await?;
    let accounts = state.storage.list_provider_accounts(query.provider).await?;
    Ok(Json(accounts))
}

async fn create_provider_account(
    State(state): State<AdminApiState>,
    headers: HeaderMap,
    Json(payload): Json<CreateProviderAccountRequest>,
) -> Result<(StatusCode, Json<ProviderAccountSummary>), AdminApiError> {
    let admin_identity = authorize_with_identity(&headers, &state.storage).await?;
    let name = require_non_empty(payload.name.as_str(), "provider account name is required")?;
    let api_key = normalize_provider_account_api_key(
        payload.provider,
        payload.api_key.as_str(),
        "provider account api key is required",
    )?;
    validate_provider_urls(
        payload.provider,
        payload.reader_base_url.as_ref(),
        payload.search_base_url.as_ref(),
    )?;
    let account = ProviderAccount {
        id: payload
            .id
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| generate_account_id(payload.provider, name.as_str())),
        provider: payload.provider,
        name,
        api_key,
        base_url: normalize_optional_text(payload.base_url),
        reader_base_url: normalize_optional_text(payload.reader_base_url),
        search_base_url: normalize_optional_text(payload.search_base_url),
        enabled: payload.enabled.unwrap_or(true),
        status: if payload.enabled.unwrap_or(true) {
            ProviderAccountStatus::Active
        } else {
            ProviderAccountStatus::Disabled
        },
        last_error: None,
        cooldown_until: None,
        last_used_at: None,
        consecutive_failures: 0,
        last_status_code: None,
        weight: 100,
        last_failure_at: None,
    };
    state.storage.create_provider_account(&account).await?;
    let summary = state
        .storage
        .list_provider_accounts(Some(account.provider))
        .await?
        .into_iter()
        .find(|item| item.id == account.id)
        .ok_or_else(|| AdminApiError::NotFound(account.id.clone()))?;

    emit_audit(
        &state,
        &admin_identity,
        "create",
        "provider_account",
        Some(&account.id),
        None,
        Some(serde_json::json!({"provider": account.provider, "name": account.name})),
    )
    .await;

    info!(
        provider = %account.provider,
        provider_account_id = %account.id,
        "admin created provider account"
    );

    Ok((StatusCode::CREATED, Json(summary)))
}

async fn update_provider_account(
    State(state): State<AdminApiState>,
    Path(account_id): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<UpdateProviderAccountRequest>,
) -> Result<Json<ProviderAccountSummary>, AdminApiError> {
    let admin_identity = authorize_with_identity(&headers, &state.storage).await?;
    let name = optional_non_empty(
        payload.name.as_ref(),
        "provider account name cannot be empty",
    )?;
    let api_key = match payload.api_key.as_ref() {
        Some(value) => {
            let account = state
                .storage
                .find_provider_account(&account_id)
                .await?
                .ok_or_else(|| AdminApiError::NotFound(account_id.clone()))?;
            Some(normalize_provider_account_api_key(
                account.provider,
                value.as_str(),
                "provider account api key cannot be empty",
            )?)
        }
        None => None,
    };
    let credential_changed = api_key.is_some();
    let base_url = if payload.clear_base_url.unwrap_or(false) {
        Some(None)
    } else {
        payload.base_url.map(normalize_text)
    };
    let account = state
        .storage
        .find_provider_account(&account_id)
        .await?
        .ok_or_else(|| AdminApiError::NotFound(account_id.clone()))?;
    validate_provider_urls(
        account.provider,
        payload.reader_base_url.as_ref(),
        payload.search_base_url.as_ref(),
    )?;
    let reader_base_url = optional_text_update(
        payload.reader_base_url,
        payload.clear_reader_base_url.unwrap_or(false),
    );
    let search_base_url = optional_text_update(
        payload.search_base_url,
        payload.clear_search_base_url.unwrap_or(false),
    );
    let updated = state
        .storage
        .update_provider_account(
            &account_id,
            ProviderAccountUpdate {
                name,
                api_key,
                base_url,
                reader_base_url,
                search_base_url,
                enabled: payload.enabled,
            },
        )
        .await?;

    if !updated {
        return Err(AdminApiError::NotFound(account_id));
    }

    let summary = state
        .storage
        .list_provider_accounts(None)
        .await?
        .into_iter()
        .find(|item| item.id == account_id)
        .ok_or_else(|| AdminApiError::NotFound(account_id.clone()))?;

    emit_audit(
        &state,
        &admin_identity,
        "update",
        "provider_account",
        Some(&account_id),
        None,
        credential_changed.then(|| serde_json::json!({"credential_changed": true})),
    )
    .await;

    info!(provider_account_id = %account_id, "admin updated provider account");

    Ok(Json(summary))
}

async fn enable_provider_account(
    State(state): State<AdminApiState>,
    Path(account_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<ProviderAccountToggleResponse>, AdminApiError> {
    let admin_identity = authorize_with_identity(&headers, &state.storage).await?;
    if !state
        .storage
        .set_provider_account_enabled(&account_id, true)
        .await?
    {
        return Err(AdminApiError::NotFound(account_id));
    }

    emit_audit(
        &state,
        &admin_identity,
        "enable",
        "provider_account",
        Some(&account_id),
        None,
        None,
    )
    .await;

    Ok(Json(ProviderAccountToggleResponse {
        id: account_id,
        enabled: true,
    }))
}

async fn disable_provider_account(
    State(state): State<AdminApiState>,
    Path(account_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<ProviderAccountToggleResponse>, AdminApiError> {
    let admin_identity = authorize_with_identity(&headers, &state.storage).await?;
    if !state
        .storage
        .set_provider_account_enabled(&account_id, false)
        .await?
    {
        return Err(AdminApiError::NotFound(account_id));
    }

    emit_audit(
        &state,
        &admin_identity,
        "disable",
        "provider_account",
        Some(&account_id),
        None,
        None,
    )
    .await;

    Ok(Json(ProviderAccountToggleResponse {
        id: account_id,
        enabled: false,
    }))
}

async fn delete_provider_account(
    State(state): State<AdminApiState>,
    Path(account_id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, AdminApiError> {
    let admin_identity = authorize_with_identity(&headers, &state.storage).await?;
    if !state.storage.delete_provider_account(&account_id).await? {
        return Err(AdminApiError::NotFound(account_id));
    }

    emit_audit(
        &state,
        &admin_identity,
        "delete",
        "provider_account",
        Some(&account_id),
        None,
        None,
    )
    .await;

    info!(provider_account_id = %account_id, "admin deleted provider account");

    Ok(StatusCode::NO_CONTENT)
}

async fn list_bound_egress_proxies(
    State(state): State<AdminApiState>,
    Path(account_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Vec<EgressProxy>>, AdminApiError> {
    authorize(&headers, &state.storage).await?;
    let proxies = state.storage.list_bound_egress_proxies(&account_id).await?;
    Ok(Json(proxies))
}

async fn bind_provider_account_proxy(
    State(state): State<AdminApiState>,
    Path(account_id): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<BindProxyRequest>,
) -> Result<StatusCode, AdminApiError> {
    let admin_identity = authorize_with_identity(&headers, &state.storage).await?;
    if payload.egress_proxy_id.trim().is_empty() {
        return Err(AdminApiError::BadRequest(
            "egress proxy id is required".to_owned(),
        ));
    }
    state
        .storage
        .bind_account_proxy(&account_id, payload.egress_proxy_id.trim())
        .await?;

    emit_audit(
        &state,
        &admin_identity,
        "bind_proxy",
        "provider_account",
        Some(&account_id),
        None,
        Some(serde_json::json!({"egress_proxy_id": payload.egress_proxy_id.trim()})),
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

fn normalize_provider_account_api_key(
    provider: ProviderId,
    value: &str,
    message: &str,
) -> Result<String, AdminApiError> {
    let value = value.trim();
    if value.is_empty() {
        if matches!(provider, ProviderId::Firecrawl | ProviderId::Jina) {
            Ok(String::new())
        } else {
            Err(AdminApiError::BadRequest(message.to_owned()))
        }
    } else {
        Ok(value.to_owned())
    }
}

fn validate_provider_urls(
    provider: ProviderId,
    reader_base_url: Option<&String>,
    search_base_url: Option<&String>,
) -> Result<(), AdminApiError> {
    if provider != ProviderId::Jina
        && (reader_base_url.is_some() || search_base_url.is_some())
    {
        return Err(AdminApiError::BadRequest(
            "reader and search base urls are only supported for jina accounts".to_owned(),
        ));
    }
    Ok(())
}

fn optional_text_update(value: Option<String>, clear: bool) -> Option<Option<String>> {
    if clear {
        Some(None)
    } else {
        value.map(normalize_text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_blank_jina_api_key() {
        let value = normalize_provider_account_api_key(
            ProviderId::Jina,
            "   ",
            "provider account api key is required",
        )
        .expect("jina keyless account should be allowed");

        assert_eq!(value, "");
    }

    #[test]
    fn allows_blank_firecrawl_api_key() {
        let value = normalize_provider_account_api_key(
            ProviderId::Firecrawl,
            "   ",
            "provider account api key is required",
        )
        .expect("firecrawl keyless account should be allowed");

        assert_eq!(value, "");
    }

    #[test]
    fn rejects_blank_non_keyless_provider_api_key() {
        let error = normalize_provider_account_api_key(
            ProviderId::Exa,
            "   ",
            "provider account api key is required",
        )
        .expect_err("non-keyless provider account should be rejected");

        assert!(matches!(error, AdminApiError::BadRequest(_)));
    }
}
