export type ProviderId = 'exa' | 'tavily' | 'firecrawl' | 'jina'

export type ProviderAccountStatus = 'active' | 'cooldown' | 'disabled'

export type EgressProxyKind = 'http' | 'https' | 'socks4' | 'socks5' | 'socks5h'

export type EgressProxyStatus = 'active' | 'cooldown' | 'disabled'

export type ProviderAsyncJobState =
  | 'pending'
  | 'running'
  | 'completed'
  | 'failed'
  | 'cancelled'

export type ProviderAccountSummary = {
  id: string
  provider: ProviderId
  name: string
  base_url: string | null
  reader_base_url: string | null
  search_base_url: string | null
  enabled: boolean
  status: ProviderAccountStatus
  last_error: string | null
  cooldown_until: string | null
  last_used_at: string | null
  consecutive_failures: number
  last_status_code: number | null
  weight: number
  created_at: string
  updated_at: string
}

export type EgressProxySummary = {
  id: string
  name: string
  kind: EgressProxyKind
  proxy_url: string
  region: string | null
  enabled: boolean
  status: EgressProxyStatus
  last_error: string | null
  cooldown_until: string | null
  last_used_at: string | null
  consecutive_failures: number
  created_at: string
  updated_at: string
}

export type EgressProxyTestResult = {
  proxy_id: string
  ok: boolean
  target_url: string
  status_code: number | null
  latency_ms: number
  message: string
  response_excerpt: string | null
}

export type RequestLogRecord = {
  id: string
  tenant_id: string | null
  platform_api_key_id: string | null
  provider: ProviderId
  provider_account_id: string | null
  egress_proxy_id: string | null
  method: string
  route: string
  upstream_url: string
  status_code: number | null
  latency_ms: number | null
  failure_kind: string | null
  failure_message: string | null
  created_at: string
}

export type ProviderAsyncJobRecord = {
  id: string
  tenant_id: string | null
  request_log_id: string | null
  provider: ProviderId
  provider_account_id: string | null
  egress_proxy_id: string | null
  route: string
  upstream_job_id: string
  state: ProviderAsyncJobState
  last_status_code: number | null
  last_error: string | null
  poll_attempts: number
  next_poll_at: string | null
  settled_at: string | null
  metadata: Record<string, unknown>
  webhook_secret: string | null
  created_at: string
  updated_at: string
}

export type ReconcileReport = {
  provider: string
  scanned: number
  progressed: number
  settled: number
  failed: number
}

export type AlertRuleRecord = {
  id: string
  name: string
  kind: string
  threshold_value: number
  webhook_url: string
  enabled: boolean
  last_triggered_at: string | null
  created_at: string
  updated_at: string
}

export type AlertEventRecord = {
  id: string
  alert_rule_id: string | null
  kind: string
  message: string
  metadata: Record<string, unknown>
  created_at: string
}

export type AdminAuditLogRecord = {
  id: string
  admin_identity: string
  action: string
  resource_type: string
  resource_id: string | null
  old_value: unknown | null
  new_value: unknown | null
  created_at: string
}

export type ProviderRequestReport = {
  provider: string
  total_requests: number
  success_count: number
  error_count: number
  avg_latency_ms: number | null
}

export type AccountHealthReport = {
  id: string
  provider: string
  name: string
  enabled: boolean
  status: string
  consecutive_failures: number
  weight: number
  last_used_at: string | null
  last_error: string | null
}

export type PlatformApiKeyRecord = {
  id: string
  name: string
  key_prefix: string
  quota: number
  request_count: number
  created_at: string
  revoked_at: string | null
}
