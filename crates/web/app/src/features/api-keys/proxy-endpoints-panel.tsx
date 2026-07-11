import { useState } from 'react'
import type { ProviderId } from '../../types'
import { CheckIcon, CopyIcon, CustomSelect, Panel, PROVIDER_IDS, ProviderTag, getProviderLabel } from '../../ui/shared'

function buildEndpoint(baseOrigin: string, provider: ProviderId) {
  return baseOrigin ? `${baseOrigin}/${provider}` : `/${provider}`
}

function buildRouteEntries(baseOrigin: string, provider: ProviderId) {
  if (provider === 'jina') {
    const jinaBase = buildEndpoint(baseOrigin, provider)

    return [
      {
        id: 'reader',
        label: 'Reader',
        endpoint: `${jinaBase}/r`,
      },
      {
        id: 'search',
        label: 'Search',
        endpoint: `${jinaBase}/s`,
      },
    ]
  }

  return [
    {
      id: provider,
      label: getProviderLabel(provider),
      endpoint: buildEndpoint(baseOrigin, provider),
    },
  ]
}

export function ProxyEndpointsPanel({
  baseOrigin,
  copiedId,
  onCopyEndpoint,
  t,
}: {
  baseOrigin: string
  copiedId: string | null
  onCopyEndpoint: (endpoint: string, copyId: string) => void
  t: (key: string, values?: Record<string, string | number>) => string
}) {
  const normalizedOrigin = baseOrigin.trim().replace(/\/+$/, '')
  const [selectedProvider, setSelectedProvider] = useState<ProviderId>('firecrawl')
  const description = normalizedOrigin
    ? t('api_keys.endpoints_desc', { origin: normalizedOrigin })
    : t('api_keys.endpoints_desc_fallback')
  const routeEntries = buildRouteEntries(normalizedOrigin, selectedProvider)
  const authenticationHint = {
    exa: 'api_keys.endpoints_hint_exa',
    tavily: 'api_keys.endpoints_hint_tavily',
    firecrawl: 'api_keys.endpoints_hint_bearer',
    jina: 'api_keys.endpoints_hint_jina',
  }[selectedProvider]

  return (
    <Panel title={t('api_keys.endpoints_title')} description={description}>
      <div className="api-endpoints-toolbar">
        <div className="api-endpoints-provider">
          <span className="api-key-hint">{t('api_keys.provider_label')}</span>
          <CustomSelect
            value={selectedProvider}
            onChange={(value) => setSelectedProvider(value as ProviderId)}
            ariaLabel={t('api_keys.provider_label')}
            options={PROVIDER_IDS.map((provider) => ({
              value: provider,
              label: getProviderLabel(provider),
            }))}
          />
        </div>
      </div>
      <div className="mini-table api-endpoints-list">
        {routeEntries.map((entry) => {
          const copyId = `endpoint:${selectedProvider}:${entry.id}`

          return (
            <div key={entry.id} className="mini-row api-endpoint-row">
              <div className="api-endpoint-meta">
                <ProviderTag provider={selectedProvider} />
                <strong>{entry.label}</strong>
              </div>
              <code className="api-endpoint-url">{entry.endpoint}</code>
              <button
                type="button"
                className={`copy-btn copy-btn-compact${copiedId === copyId ? ' is-copied' : ''}`}
                onClick={() => onCopyEndpoint(entry.endpoint, copyId)}
              >
                {copiedId === copyId ? (
                  <>
                    <CheckIcon />
                    {t('common.copied')}
                  </>
                ) : (
                  <>
                    <CopyIcon />
                    {t('common.copy')}
                  </>
                )}
              </button>
            </div>
          )
        })}
      </div>
      <p className="api-key-hint">
        {t(authenticationHint)}
      </p>
    </Panel>
  )
}
