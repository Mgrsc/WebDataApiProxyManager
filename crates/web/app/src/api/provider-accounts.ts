import type { EgressProxySummary, ProviderAccountSummary, ProviderId } from '../types'
import { buildSearchParams, request } from './core'

export const providerAccountsApi = {
  listProviderAccounts(token: string, provider?: ProviderId) {
    return request<ProviderAccountSummary[]>(`/provider-accounts${buildSearchParams({ provider })}`, { token })
  },
  createProviderAccount(
    token: string,
    payload: {
      provider: ProviderId
      name: string
      api_key: string
      base_url?: string
      reader_base_url?: string
      search_base_url?: string
      enabled?: boolean
    },
  ) {
    return request<ProviderAccountSummary>('/provider-accounts', {
      method: 'POST',
      token,
      body: payload,
    })
  },
  setProviderAccountEnabled(token: string, accountId: string, enabled: boolean) {
    return request(`/provider-accounts/${accountId}/${enabled ? 'enable' : 'disable'}`, {
      method: 'POST',
      token,
    })
  },
  deleteProviderAccount(token: string, accountId: string) {
    return request<void>(`/provider-accounts/${accountId}`, {
      method: 'DELETE',
      token,
    })
  },
  updateProviderAccount(
    token: string,
    accountId: string,
    payload: {
      name?: string
      api_key?: string
      base_url?: string
      clear_base_url?: boolean
      reader_base_url?: string
      clear_reader_base_url?: boolean
      search_base_url?: string
      clear_search_base_url?: boolean
      enabled?: boolean
    },
  ) {
    return request<ProviderAccountSummary>(`/provider-accounts/${accountId}`, {
      method: 'PATCH',
      token,
      body: payload,
    })
  },
  listBoundEgressProxies(token: string, accountId: string) {
    return request<EgressProxySummary[]>(`/provider-accounts/${accountId}/egress-proxies`, { token })
  },
  bindProxy(token: string, accountId: string, egressProxyId: string) {
    return request(`/provider-accounts/${accountId}/bind-proxy`, {
      method: 'POST',
      token,
      body: { egress_proxy_id: egressProxyId },
    })
  },
}
