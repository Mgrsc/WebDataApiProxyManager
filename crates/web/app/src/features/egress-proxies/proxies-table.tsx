import type { EgressProxySummary, EgressProxyTestResult } from '../../types'
import { EmptyState, ErrorBanner, Panel, formatMaybe } from '../../ui/shared'
import { EgressProxyRow } from './proxy-row'

export function ProxiesTable({
  title,
  description,
  proxies,
  editingId,
  editName,
  editProxyUrl,
  updatePending,
  togglePending,
  testingId,
  testResults,
  updateError,
  toggleError,
  testError,
  onEditNameChange,
  onEditProxyUrlChange,
  onSaveEdit,
  onCancelEdit,
  onStartEdit,
  onToggleEnabled,
  onTestProxy,
  t,
}: {
  title: string
  description: string
  proxies: EgressProxySummary[]
  editingId: string | null
  editName: string
  editProxyUrl: string
  updatePending: boolean
  togglePending: boolean
  testingId: string | null
  testResults: Record<string, EgressProxyTestResult>
  updateError?: string
  toggleError?: string
  testError?: string
  onEditNameChange: (value: string) => void
  onEditProxyUrlChange: (value: string) => void
  onSaveEdit: (proxyId: string) => void
  onCancelEdit: () => void
  onStartEdit: (proxy: EgressProxySummary) => void
  onToggleEnabled: (proxyId: string, enabled: boolean) => void
  onTestProxy: (proxyId: string) => void
  t: (key: string, values?: Record<string, string | number>) => string
}) {
  return (
    <Panel title={title} description={description}>
      {proxies.length === 0 ? (
        <EmptyState title={t('proxies.no_proxies')} body={t('proxies.no_proxies_desc')} />
      ) : (
        <div className="table-scroll">
          <table className="data-table proxies-table">
            <thead>
              <tr>
                <th>{t('table.name')}</th>
                <th>{t('table.kind')}</th>
                <th>{t('table.status')}</th>
                <th>{t('table.failures')}</th>
                <th>{t('table.target')}</th>
                <th>{t('table.actions')}</th>
              </tr>
            </thead>
            <tbody>
              {proxies.map((proxy) => (
                <EgressProxyRow
                  key={proxy.id}
                  proxy={proxy}
                  editingId={editingId}
                  editName={editName}
                  editProxyUrl={editProxyUrl}
                  updatePending={updatePending}
                  togglePending={togglePending}
                  testingId={testingId}
                  testResult={testResults[proxy.id]}
                  onEditNameChange={onEditNameChange}
                  onEditProxyUrlChange={onEditProxyUrlChange}
                  onSaveEdit={onSaveEdit}
                  onCancelEdit={onCancelEdit}
                  onStartEdit={onStartEdit}
                  onToggleEnabled={onToggleEnabled}
                  onTestProxy={onTestProxy}
                  t={t}
                />
              ))}
            </tbody>
          </table>
        </div>
      )}
      {proxies.some((proxy) => proxy.last_error) ? (
        <div className="footnote-grid">
          {proxies
            .filter((proxy) => proxy.last_error)
            .slice(0, 4)
            .map((proxy) => (
              <div key={proxy.id} className="footnote">
                <strong>{proxy.name}</strong>
                <span>{formatMaybe(proxy.last_error)}</span>
              </div>
            ))}
        </div>
      ) : null}
      {updateError ? <ErrorBanner message={updateError} /> : null}
      {toggleError ? <ErrorBanner message={toggleError} /> : null}
      {testError ? <ErrorBanner message={testError} /> : null}
    </Panel>
  )
}
