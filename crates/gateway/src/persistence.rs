#[derive(Clone)]
struct PersistenceBundle {
    request_log: RequestLogInsert,
    async_job: Option<ProviderAsyncJobInsert>,
}

struct BuildRequestLogArgs<'a> {
    request_id: Uuid,
    tenant_id: Option<String>,
    platform_api_key_id: Option<String>,
    provider: ProviderId,
    provider_account_id: Option<String>,
    egress_proxy_id: Option<String>,
    method: String,
    route: String,
    upstream_url: String,
    status_code: Option<i64>,
    latency_ms: Option<i64>,
    failure_kind: Option<String>,
    failure_message: Option<String>,
    request_headers: HeaderValues,
    response_headers: HeaderValues,
    request_body_bytes: &'a [u8],
    response_body_bytes: Option<&'a [u8]>,
}

#[derive(Clone, Copy)]
struct PersistenceContext<'a> {
    request_id: Uuid,
    provider_id: ProviderId,
    provider_account_id: Option<&'a str>,
    egress_proxy_id: Option<&'a str>,
    egress_mode: &'a str,
    egress_target: &'a str,
}

async fn finalize_upstream_response(
    state: &GatewayState,
    args: FinalizeUpstreamResponseArgs<'_>,
    upstream: UpstreamResponseData,
) -> Result<Response, GatewayError> {
    let egress_mode = if args.egress_proxy.is_some() {
        "proxy"
    } else {
        "direct"
    };
    let egress_target = args
        .egress_proxy
        .as_ref()
        .map(|value| summarize_proxy_url(&value.proxy_url))
        .unwrap_or_else(|| "direct".to_owned());
    let response_headers = flatten_headers(&upstream.headers);
    let request_log = build_request_log(
        &state.request_log_capture,
        BuildRequestLogArgs {
            request_id: args.request_envelope.request_id,
            tenant_id: Some((*state.default_tenant_id).clone()),
            platform_api_key_id: Some(args.platform_api_key.id.clone()),
            provider: args.provider_id,
            provider_account_id: Some(args.account.id.clone()),
            egress_proxy_id: args.egress_proxy.map(|value| value.id.clone()),
            method: args.request_envelope.method.clone(),
            route: format_route(args.provider, args.request_envelope.rest_path.as_str()),
            upstream_url: args.plan_url.to_owned(),
            status_code: Some(i64::from(upstream.status.as_u16())),
            latency_ms: args.latency_ms,
            failure_kind: None,
            failure_message: None,
            request_headers: args.request_envelope.headers.clone(),
            response_headers,
            request_body_bytes: args.request_envelope.body.as_slice(),
            response_body_bytes: Some(upstream.body.as_ref()),
        },
    );
    let mut persistence_bundle = PersistenceBundle {
        request_log: request_log.clone(),
        async_job: None,
    };

    if args.provider_id == ProviderId::Firecrawl {
        match detect_async_job(
            args.upstream_path,
            upstream.status.as_u16(),
            upstream.body.as_ref(),
        ) {
            Ok(Some(async_job)) => {
                match state
                    .storage
                    .find_provider_async_job_by_upstream_id(
                        args.provider_id,
                        &async_job.upstream_job_id,
                    )
                    .await
                {
                    Ok(Some(existing_job)) => {
                        info!(
                            request_id = %request_log.id,
                            provider = %args.provider_id,
                            async_job_id = existing_job.id,
                            upstream_job_id = %async_job.upstream_job_id,
                            egress_mode,
                            egress_target = %egress_target,
                            "firecrawl async job already tracked"
                        );
                    }
                    Ok(None) => {
                        let insert = ProviderAsyncJobInsert {
                            id: Uuid::now_v7(),
                            tenant_id: Some((*state.default_tenant_id).clone()),
                            request_log_id: Some(request_log.id),
                            provider: args.provider_id,
                            provider_account_id: Some(args.account.id.clone()),
                            egress_proxy_id: args.egress_proxy.map(|value| value.id.clone()),
                            route: async_job.route,
                            upstream_job_id: async_job.upstream_job_id,
                            state: ProviderAsyncJobState::Pending,
                            last_status_code: Some(i64::from(upstream.status.as_u16())),
                            last_error: None,
                            poll_attempts: 0,
                            next_poll_at: None,
                            settled_at: None,
                            metadata: async_job.metadata,
                            webhook_secret: args.webhook_secret.clone(),
                        };
                        persistence_bundle.async_job = Some(insert);
                    }
                    Err(storage_error) => {
                        error!(
                            request_id = %request_log.id,
                            provider = %args.provider_id,
                            provider_account_id = %args.account.id,
                            egress_mode,
                            egress_target = %egress_target,
                            error = %storage_error,
                            "failed to query firecrawl async job state"
                        );
                    }
                }
            }
            Ok(None) => {}
            Err(provider_error) => {
                error!(
                    request_id = %request_log.id,
                    provider = %args.provider_id,
                    provider_account_id = %args.account.id,
                    egress_mode,
                    egress_target = %egress_target,
                    error = %provider_error,
                    "failed to detect firecrawl async job"
                );
            }
        }
    }

    enqueue_persistence(
        state,
        persistence_bundle,
        PersistenceContext {
            request_id: request_log.id,
            provider_id: args.provider_id,
            provider_account_id: Some(args.account.id.as_str()),
            egress_proxy_id: args.egress_proxy.map(|value| value.id.as_str()),
            egress_mode,
            egress_target: egress_target.as_str(),
        },
    )
    .await;

    if !args.outcome_recorded
        && let Err(storage_error) = record_provider_account_outcome(
            state,
            &args.account.id,
            args.account.consecutive_failures,
            upstream.status,
            args.response_class.disposition,
        )
        .await
    {
        error!(
            request_id = %request_log.id,
            provider = %args.provider_id,
            provider_account_id = %args.account.id,
            egress_mode,
            egress_target = %egress_target,
            error = %storage_error,
            "failed to update provider account outcome"
        );
    }

    if let Some(egress_proxy) = args.egress_proxy
        && let Err(storage_error) = state
            .storage
            .record_egress_proxy_success(&egress_proxy.id)
            .await
    {
        error!(
            request_id = %request_log.id,
            provider = %args.provider_id,
            provider_account_id = %args.account.id,
            egress_proxy_id = %egress_proxy.id,
            egress_mode,
            egress_target = %egress_target,
            error = %storage_error,
            "failed to update egress proxy outcome"
        );
    }

    let status = upstream.status;
    let response = build_response(upstream, args.provider_id)?;

    info!(
        request_id = %args.request_envelope.request_id,
        provider = %args.provider_id,
        provider_account_id = %args.account.id,
        egress_proxy_id = args.egress_proxy.map(|value| value.id.as_str()),
        egress_mode,
        egress_target = %egress_target,
        status = status.as_u16(),
        latency_ms = args.latency_ms,
        retryable = args.response_class.retryable,
        disposition = ?args.response_class.disposition,
        selection_reason = %args.selection_reason,
        attempts = args.attempts,
        "proxied upstream request"
    );

    Ok(response)
}

fn classify_transport_failure(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else if error.is_body() {
        "body"
    } else if error.is_builder() {
        "request_build"
    } else if error.is_redirect() {
        "redirect"
    } else if error.is_decode() {
        "decode"
    } else {
        "request"
    }
}

fn classify_gateway_failure(error: &GatewayError) -> &'static str {
    match error {
        GatewayError::Http(inner) => classify_transport_failure(inner),
        GatewayError::Provider(_) => "provider",
        GatewayError::Scheduler(_) => "scheduler",
        GatewayError::ResponseBuild(_) => "response_build",
        GatewayError::Storage(_) => "storage",
        GatewayError::WebhookStorage(_) => "storage",
        GatewayError::ClientPoolPoisoned => "client_pool",
        GatewayError::ProviderUnavailable(_) => "provider_unavailable",
        GatewayError::MissingAuthorization
        | GatewayError::InvalidAuthorization
        | GatewayError::InvalidPlatformKey
        | GatewayError::QuotaExceeded => "request",
    }
}

async fn persist_bundle(
    storage: &StorageService,
    bundle: PersistenceBundle,
) -> Result<(), wdapm_storage::StorageError> {
    storage.insert_request_log(&bundle.request_log).await?;
    if let Some(async_job) = &bundle.async_job {
        storage.insert_provider_async_job(async_job).await?;
    }
    Ok(())
}

async fn enqueue_persistence(
    state: &GatewayState,
    bundle: PersistenceBundle,
    context: PersistenceContext<'_>,
) {
    let async_job_id = bundle.async_job.as_ref().map(|value| value.id);
    let upstream_job_id = bundle
        .async_job
        .as_ref()
        .map(|value| value.upstream_job_id.as_str());

    match state.try_enqueue_persistence(bundle.clone()) {
        Ok(()) => {
            if let Some(async_job_id) = async_job_id {
                info!(
                    request_id = %context.request_id,
                    provider = %context.provider_id,
                    provider_account_id = context.provider_account_id,
                    async_job_id = %async_job_id,
                    upstream_job_id,
                    egress_proxy_id = context.egress_proxy_id,
                    egress_mode = context.egress_mode,
                    egress_target = context.egress_target,
                    "persistence bundle enqueued"
                );
            }
        }
        Err(queue_error) => match persist_bundle(&state.storage, bundle).await {
            Ok(()) => {
                warn!(
                    request_id = %context.request_id,
                    provider = %context.provider_id,
                    provider_account_id = context.provider_account_id,
                    egress_proxy_id = context.egress_proxy_id,
                    egress_mode = context.egress_mode,
                    egress_target = context.egress_target,
                    error = %queue_error,
                    "persistence queue saturated; fell back to synchronous write"
                );
            }
            Err(storage_error) => {
                error!(
                    request_id = %context.request_id,
                    provider = %context.provider_id,
                    provider_account_id = context.provider_account_id,
                    egress_proxy_id = context.egress_proxy_id,
                    egress_mode = context.egress_mode,
                    egress_target = context.egress_target,
                    queue_error = %queue_error,
                    error = %storage_error,
                    "failed to persist bundle after queue saturation"
                );
            }
        },
    }
}

fn build_request_log(
    config: &RequestLogCaptureConfig,
    args: BuildRequestLogArgs<'_>,
) -> RequestLogInsert {
    let capture_bodies = should_capture_bodies(
        config,
        args.status_code,
        args.latency_ms,
        args.failure_kind.as_deref(),
    );
    RequestLogInsert {
        id: args.request_id,
        tenant_id: args.tenant_id,
        platform_api_key_id: args.platform_api_key_id,
        provider: args.provider,
        provider_account_id: args.provider_account_id,
        egress_proxy_id: args.egress_proxy_id,
        method: args.method,
        route: args.route,
        upstream_url: args.upstream_url,
        status_code: args.status_code,
        latency_ms: args.latency_ms,
        failure_kind: args.failure_kind,
        failure_message: args.failure_message,
        request_headers: args.request_headers,
        response_headers: args.response_headers,
        request_body: capture_bodies
            .then(|| capture_text_body(args.request_body_bytes, config.body_max_bytes))
            .flatten(),
        response_body: capture_bodies
            .then(|| {
                args.response_body_bytes
                    .and_then(|body| capture_text_body(body, config.body_max_bytes))
            })
            .flatten(),
    }
}

fn should_capture_bodies(
    config: &RequestLogCaptureConfig,
    status_code: Option<i64>,
    latency_ms: Option<i64>,
    failure_kind: Option<&str>,
) -> bool {
    if config.body_max_bytes == 0 {
        return false;
    }

    let is_failure = failure_kind.is_some()
        || status_code
            .map(|value| !(200..=299).contains(&value))
            .unwrap_or(false);
    let is_slow = latency_ms
        .and_then(|value| u64::try_from(value).ok())
        .map(|value| value >= config.slow_request_threshold_ms)
        .unwrap_or(false);

    match config.mode {
        RequestLogCaptureMode::MetadataOnly => false,
        RequestLogCaptureMode::FailuresOnly => is_failure,
        RequestLogCaptureMode::FailuresAndSlow => is_failure || is_slow,
    }
}

fn capture_text_body(body: &[u8], max_bytes: usize) -> Option<String> {
    if body.is_empty() {
        return None;
    }

    let limit = body.len().min(max_bytes.max(1));
    let truncated = body.len() > limit;
    let mut value = std::str::from_utf8(&body[..limit]).ok()?.to_owned();
    if truncated {
        value.push_str("\n...[truncated]");
    }
    Some(value)
}
