import type { EgressProxySummary, EgressProxyTestResult } from '../../types'
import {
  ActionDropdown,
  StatusBadge,
  formatManagedStatus,
  toneFromStatus,
} from '../../ui/shared'

export function EgressProxyRow({
  proxy,
  editingId,
  editName,
  editProxyUrl,
  updatePending,
  togglePending,
  testingId,
  testResult,
  onEditNameChange,
  onEditProxyUrlChange,
  onSaveEdit,
  onCancelEdit,
  onStartEdit,
  onToggleEnabled,
  onTestProxy,
  t,
}: {
  proxy: EgressProxySummary
  editingId: string | null
  editName: string
  editProxyUrl: string
  updatePending: boolean
  togglePending: boolean
  testingId: string | null
  testResult?: EgressProxyTestResult
  onEditNameChange: (value: string) => void
  onEditProxyUrlChange: (value: string) => void
  onSaveEdit: (proxyId: string) => void
  onCancelEdit: () => void
  onStartEdit: (proxy: EgressProxySummary) => void
  onToggleEnabled: (proxyId: string, enabled: boolean) => void
  onTestProxy: (proxyId: string) => void
  t: (key: string, values?: Record<string, string | number>) => string
}) {
  const isEditing = editingId === proxy.id
  const isTesting = testingId === proxy.id
  const compactTarget = summarizeProxyTarget(proxy.proxy_url)
  const testLabel = testResult
    ? [
        testResult.ok ? t('proxies.test_success') : t('proxies.test_failed'),
        `${testResult.latency_ms}ms`,
        testResult.status_code
          ? `HTTP ${testResult.status_code}`
          : !testResult.ok
            ? testResult.message
            : null,
      ]
        .filter(Boolean)
        .join(' · ')
    : null

  return (
    <tr>
      <td>
        {isEditing ? (
          <input
            value={editName}
            onChange={(event) => onEditNameChange(event.target.value)}
            placeholder={proxy.name}
          />
        ) : (
          <div className="cell-stack">
            <strong>{proxy.name}</strong>
            <span>{proxy.id}</span>
          </div>
        )}
      </td>
      <td>
        <span className="tag">{proxy.kind}</span>
      </td>
      <td>
        <StatusBadge tone={toneFromStatus(proxy.status)}>
          {formatManagedStatus(proxy.status, t)}
        </StatusBadge>
      </td>
      <td>{proxy.consecutive_failures}</td>
      <td className="mono-cell proxy-target-cell">
        {isEditing ? (
          <input
            value={editProxyUrl}
            onChange={(event) => onEditProxyUrlChange(event.target.value)}
            placeholder={proxy.proxy_url}
          />
        ) : (
          <div className="proxy-target-stack">
            <span className="proxy-target-value" title={proxy.proxy_url}>
              {compactTarget}
            </span>
            {isTesting ? (
              <span className="proxy-test-feedback">{t('common.testing')}</span>
            ) : testLabel ? (
              <span
                className={`proxy-test-feedback${testResult?.ok ? ' is-success' : ' is-danger'}`}
                title={testResult?.response_excerpt ?? testResult?.message}
              >
                {testLabel}
              </span>
            ) : null}
          </div>
        )}
      </td>
      <td>
        <div className="inline-action">
          {isEditing ? (
            <>
              <button
                type="button"
                className="primary-button"
                disabled={updatePending}
                onClick={() => onSaveEdit(proxy.id)}
              >
                {updatePending ? t('common.saving') : t('common.save')}
              </button>
              <button type="button" className="ghost-button" onClick={onCancelEdit}>
                {t('common.cancel')}
              </button>
            </>
          ) : (
            <ActionDropdown
              primaryLabel={t('common.edit')}
              onPrimaryClick={() => onStartEdit(proxy)}
              items={[
                {
                  label: isTesting ? t('common.testing') : t('common.test'),
                  disabled: isTesting,
                  onClick: () => onTestProxy(proxy.id),
                },
                {
                  label: proxy.enabled ? t('common.disable') : t('common.enable'),
                  danger: proxy.enabled,
                  disabled: togglePending,
                  onClick: () => onToggleEnabled(proxy.id, !proxy.enabled),
                },
              ]}
            />
          )}
        </div>
      </td>
    </tr>
  )
}

function summarizeProxyTarget(value: string) {
  try {
    const parsed = new URL(value)
    return `${parsed.protocol}//${parsed.host}`
  } catch {
    return value
  }
}
