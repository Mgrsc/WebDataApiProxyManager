import { EmptyState, ErrorBanner, Panel, ProviderSelect, formatMaybe } from '../../ui/shared'
import type { EgressProxySummary, ProviderAccountSummary } from '../../types'
import type { AccountBulkAction, AccountSortMode, ProviderFilter } from './utils'
import { AccountsBulkBar } from './accounts-bulk-bar'
import { ProviderAccountRow } from './account-row'

export function AccountsPanel({
  token,
  title,
  description,
  providerFilter,
  selectedIds,
  sortMode,
  visibleAccounts,
  accounts,
  proxies,
  bindSelections,
  bulkProxyId,
  allVisibleSelected,
  editingId,
  editName,
  editBaseUrl,
  editReaderBaseUrl,
  editSearchBaseUrl,
  editApiKey,
  bindPending,
  bulkPending,
  updatePending,
  togglePending,
  deletePending,
  updateError,
  bindError,
  bulkError,
  onProviderFilterChange,
  onToggleSort,
  onToggleSelectAllVisible,
  onToggleSelection,
  onBulkProxyChange,
  onRunBulkAction,
  onEditNameChange,
  onEditBaseUrlChange,
  onEditReaderBaseUrlChange,
  onEditSearchBaseUrlChange,
  onEditApiKeyChange,
  onBindSelectionChange,
  onBindAccount,
  onSaveEdit,
  onCancelEdit,
  onStartEdit,
  onToggleEnabled,
  onDelete,
  t,
}: {
  token: string
  title: string
  description: string
  providerFilter: ProviderFilter
  selectedIds: string[]
  sortMode: AccountSortMode
  visibleAccounts: ProviderAccountSummary[]
  accounts: ProviderAccountSummary[]
  proxies: EgressProxySummary[]
  bindSelections: Record<string, string>
  bulkProxyId: string
  allVisibleSelected: boolean
  editingId: string | null
  editName: string
  editBaseUrl: string
  editReaderBaseUrl: string
  editSearchBaseUrl: string
  editApiKey: string
  bindPending: boolean
  bulkPending: boolean
  updatePending: boolean
  togglePending: boolean
  deletePending: boolean
  updateError?: string
  bindError?: string
  bulkError?: string
  onProviderFilterChange: (value: ProviderFilter) => void
  onToggleSort: () => void
  onToggleSelectAllVisible: (checked: boolean) => void
  onToggleSelection: (accountId: string, checked: boolean) => void
  onBulkProxyChange: (value: string) => void
  onRunBulkAction: (action: AccountBulkAction) => void
  onEditNameChange: (value: string) => void
  onEditBaseUrlChange: (value: string) => void
  onEditReaderBaseUrlChange: (value: string) => void
  onEditSearchBaseUrlChange: (value: string) => void
  onEditApiKeyChange: (value: string) => void
  onBindSelectionChange: (accountId: string, value: string) => void
  onBindAccount: (accountId: string) => void
  onSaveEdit: (accountId: string) => void
  onCancelEdit: () => void
  onStartEdit: (account: ProviderAccountSummary) => void
  onToggleEnabled: (accountId: string, enabled: boolean) => void
  onDelete: (accountId: string) => void
  t: (key: string, values?: Record<string, string | number>) => string
}) {
  return (
    <Panel title={title} description={description}>
      <div className="accounts-toolbar">
        <div className="accounts-filter-bar">
          <label className="inline-field compact">
            <ProviderSelect
              value={providerFilter === 'all' ? '' : providerFilter}
              includeAll
              allLabel="ALL"
              onChange={(value) => onProviderFilterChange((value || 'all') as ProviderFilter)}
              ariaLabel={t('table.provider')}
            />
          </label>
        </div>
        {selectedIds.length > 0 ? (
          <AccountsBulkBar
            selectedCount={selectedIds.length}
            bulkProxyId={bulkProxyId}
            bulkPending={bulkPending}
            proxies={proxies}
            onBulkProxyChange={onBulkProxyChange}
            onRunBulkAction={onRunBulkAction}
            t={t}
          />
        ) : null}
      </div>

      {visibleAccounts.length === 0 ? (
        <EmptyState title={t('accounts.no_accounts')} body={t('accounts.no_accounts_desc')} />
      ) : (
        <div className="table-scroll">
          <table className="data-table accounts-table">
            <thead>
              <tr>
                <th>
                  <input
                    className="accounts-checkbox"
                    type="checkbox"
                    checked={allVisibleSelected}
                    onChange={(event) => onToggleSelectAllVisible(event.target.checked)}
                  />
                </th>
                <th>{t('table.provider')}</th>
                <th>{t('table.name')}</th>
                <th>{t('table.status')}</th>
                <th className="sortable-th" onClick={onToggleSort}>
                  {t('table.failures')}
                  <svg className={`sort-icon${sortMode === 'failures_desc' ? ' sort-active' : ''}`} viewBox="0 0 16 16" width="12" height="12" fill="currentColor">
                    <path d="M8 3.5a.5.5 0 01.354.146l3.5 3.5a.5.5 0 11-.708.708L8 4.707 4.854 7.854a.5.5 0 11-.708-.708l3.5-3.5A.5.5 0 018 3.5zm0 9a.5.5 0 01-.354-.146l-3.5-3.5a.5.5 0 11.708-.708L8 11.293l3.146-3.147a.5.5 0 01.708.708l-3.5 3.5A.5.5 0 018 12.5z" />
                  </svg>
                </th>
                <th>{t('table.proxy_binding')}</th>
                <th>{t('table.actions')}</th>
              </tr>
            </thead>
            <tbody>
              {visibleAccounts.map((account) => (
                <ProviderAccountRow
                  key={account.id}
                  token={token}
                  account={account}
                  selected={selectedIds.includes(account.id)}
                  editingId={editingId}
                  editName={editName}
                  editBaseUrl={editBaseUrl}
                  editReaderBaseUrl={editReaderBaseUrl}
                  editSearchBaseUrl={editSearchBaseUrl}
                  editApiKey={editApiKey}
                  bindSelections={bindSelections}
                  proxies={proxies}
                  bindPending={bindPending}
                  updatePending={updatePending}
                  togglePending={togglePending}
                  deletePending={deletePending}
                  onToggleSelection={onToggleSelection}
                  onEditNameChange={onEditNameChange}
                  onEditBaseUrlChange={onEditBaseUrlChange}
                  onEditReaderBaseUrlChange={onEditReaderBaseUrlChange}
                  onEditSearchBaseUrlChange={onEditSearchBaseUrlChange}
                  onEditApiKeyChange={onEditApiKeyChange}
                  onBindSelectionChange={onBindSelectionChange}
                  onBindAccount={onBindAccount}
                  onSaveEdit={onSaveEdit}
                  onCancelEdit={onCancelEdit}
                  onStartEdit={onStartEdit}
                  onToggleEnabled={onToggleEnabled}
                  onDelete={onDelete}
                  t={t}
                />
              ))}
            </tbody>
          </table>
        </div>
      )}

      {accounts.some((account) => account.last_error) ? (
        <div className="footnote-grid">
          {accounts
            .filter((account) => account.last_error)
            .slice(0, 4)
            .map((account) => (
              <div key={account.id} className="footnote">
                <strong>{account.name}</strong>
                <span>{formatMaybe(account.last_error)}</span>
              </div>
            ))}
        </div>
      ) : null}

      {updateError ? <ErrorBanner message={updateError} /> : null}
      {bindError ? <ErrorBanner message={bindError} /> : null}
      {bulkError ? <ErrorBanner message={bulkError} /> : null}
    </Panel>
  )
}
