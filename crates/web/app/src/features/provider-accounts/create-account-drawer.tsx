import { Drawer, ErrorBanner, ProviderSelect } from '../../ui/shared'
import { getProviderDefaultBaseUrl, getProviderLabel } from '../../lib/provider'
import type { ProviderId } from '../../types'

export function CreateAccountDrawer({
  open,
  onClose,
  provider,
  providerAutoDetected,
  detectedProvider,
  name,
  apiKeyInput,
  batchApiKeysInput,
  baseUrl,
  readerBaseUrl,
  searchBaseUrl,
  batchApiKeysLength,
  detectedBatchCount,
  unknownBatchCount,
  createPending,
  bulkPending,
  createError,
  bulkError,
  onProviderChange,
  onNameChange,
  onApiKeyChange,
  onBatchApiKeysChange,
  onBaseUrlChange,
  onReaderBaseUrlChange,
  onSearchBaseUrlChange,
  onCreate,
  onBulkCreate,
  t,
}: {
  open: boolean
  onClose: () => void
  provider: ProviderId
  providerAutoDetected: boolean
  detectedProvider: ProviderId | null
  name: string
  apiKeyInput: string
  batchApiKeysInput: string
  baseUrl: string
  readerBaseUrl: string
  searchBaseUrl: string
  batchApiKeysLength: number
  detectedBatchCount: number
  unknownBatchCount: number
  createPending: boolean
  bulkPending: boolean
  createError?: string
  bulkError?: string
  onProviderChange: (provider: ProviderId) => void
  onNameChange: (value: string) => void
  onApiKeyChange: (value: string) => void
  onBatchApiKeysChange: (value: string) => void
  onBaseUrlChange: (value: string) => void
  onReaderBaseUrlChange: (value: string) => void
  onSearchBaseUrlChange: (value: string) => void
  onCreate: () => void
  onBulkCreate: () => void
  t: (key: string, values?: Record<string, string | number>) => string
}) {
  const apiKeyOptional = provider === 'firecrawl' || provider === 'jina'
  const apiKeyNoteKey =
    provider === 'firecrawl'
      ? 'accounts.api_key_optional_firecrawl'
      : provider === 'jina'
        ? 'accounts.api_key_optional_jina'
        : 'accounts.api_key_prefix_examples'
  const canCreate = Boolean(name.trim()) && (Boolean(apiKeyInput.trim()) || apiKeyOptional)
  const defaultBaseUrl = getProviderDefaultBaseUrl(provider)

  return (
    <Drawer open={open} onClose={onClose} title={t('accounts.create')}>
      <p className="panel-copy">{t('accounts.create_desc')}</p>
      <div className="stack-form stack-form-spaced">
        <label className="field">
          <span>{t('table.provider')}</span>
          <ProviderSelect
            value={provider}
            disabled={providerAutoDetected && batchApiKeysLength === 0}
            onChange={(value) => onProviderChange(value as ProviderId)}
          />
          <p className="field-note">
            {batchApiKeysLength > 0
              ? unknownBatchCount > 0
                ? `${t('accounts.batch_unknown_prefix')} ${unknownBatchCount} · ${t('accounts.batch_fallback_provider')}: ${getProviderLabel(provider)}`
                : t('accounts.batch_all_detected')
              : detectedProvider
                ? `${t('accounts.provider_auto_detected')}: ${getProviderLabel(detectedProvider)}`
                : t('accounts.provider_manual_select')}
          </p>
        </label>
        <label className="field">
          <span>{t('table.name')}</span>
          <input value={name} onChange={(event) => onNameChange(event.target.value)} placeholder="e.g. exa-prod-01" />
        </label>
        <label className="field">
          <span>{t('table.api_key')}</span>
          <input
            type="password"
            value={apiKeyInput}
            onChange={(event) => onApiKeyChange(event.target.value)}
          />
          <p className="field-note">
            {t(apiKeyNoteKey)}
          </p>
        </label>
        <label className="field">
          <span>{t('accounts.batch_api_keys')}</span>
          <textarea
            rows={6}
            value={batchApiKeysInput}
            onChange={(event) => onBatchApiKeysChange(event.target.value)}
            placeholder={t('accounts.batch_api_keys_placeholder')}
          />
          <p className="field-note">
            {batchApiKeysLength > 0
              ? `${t('accounts.batch_total')}: ${batchApiKeysLength} · ${t('accounts.batch_detected')}: ${detectedBatchCount} · ${t('accounts.batch_unknown')}: ${unknownBatchCount}`
              : t('accounts.batch_hint')}
          </p>
        </label>
        {provider === 'jina' ? (
          <>
            <label className="field">
              <span>{t('accounts.reader_base_url')}</span>
              <input value={readerBaseUrl} onChange={(event) => onReaderBaseUrlChange(event.target.value)} placeholder="https://r.jina.ai" />
            </label>
            <label className="field">
              <span>{t('accounts.search_base_url')}</span>
              <input value={searchBaseUrl} onChange={(event) => onSearchBaseUrlChange(event.target.value)} placeholder="https://s.jina.ai" />
            </label>
          </>
        ) : (
          <label className="field">
            <span>{t('accounts.base_url_optional')}</span>
            <input value={baseUrl} onChange={(event) => onBaseUrlChange(event.target.value)} placeholder={defaultBaseUrl} />
            <p className="field-note">{t('accounts.base_url_default', { url: defaultBaseUrl })}</p>
          </label>
        )}
        {createError ? <ErrorBanner message={createError} /> : null}
        {bulkError ? <ErrorBanner message={bulkError} /> : null}
        <button
          type="button"
          className="primary-button"
          disabled={createPending || bulkPending || !canCreate}
          onClick={onCreate}
        >
          {createPending ? t('common.creating') : t('common.add_account')}
        </button>
        <button
          type="button"
          className="ghost-button"
          disabled={createPending || bulkPending || batchApiKeysLength === 0}
          onClick={onBulkCreate}
        >
          {bulkPending ? t('accounts.batch_importing') : t('accounts.batch_import')}
        </button>
      </div>
    </Drawer>
  )
}
