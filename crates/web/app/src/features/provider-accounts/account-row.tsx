import type { EgressProxySummary, ProviderAccountSummary } from '../../types'
import {
  ActionDropdown,
  CustomSelect,
  ProviderTag,
  StatusBadge,
  formatManagedStatus,
  toneFromStatus,
} from '../../ui/shared'
import { BoundProxyTags } from './bound-proxy-tags'

export function ProviderAccountRow({
  token,
  account,
  selected,
  editingId,
  editName,
  editBaseUrl,
  editReaderBaseUrl,
  editSearchBaseUrl,
  editApiKey,
  bindSelections,
  proxies,
  bindPending,
  updatePending,
  togglePending,
  deletePending,
  onToggleSelection,
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
  account: ProviderAccountSummary
  selected: boolean
  editingId: string | null
  editName: string
  editBaseUrl: string
  editReaderBaseUrl: string
  editSearchBaseUrl: string
  editApiKey: string
  bindSelections: Record<string, string>
  proxies: EgressProxySummary[]
  bindPending: boolean
  updatePending: boolean
  togglePending: boolean
  deletePending: boolean
  onToggleSelection: (accountId: string, checked: boolean) => void
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
  const isEditing = editingId === account.id

  return (
    <tr>
      <td>
        <input
          className="accounts-checkbox"
          type="checkbox"
          checked={selected}
          onChange={(event) => onToggleSelection(account.id, event.target.checked)}
        />
      </td>
      <td>
        <ProviderTag provider={account.provider} />
      </td>
      <td>
        {isEditing ? (
          <div className="cell-stack accounts-name-cell">
            <input
              value={editName}
              onChange={(event) => onEditNameChange(event.target.value)}
              placeholder={account.name}
            />
            {account.provider === 'jina' ? (
              <>
                <input className="accounts-edit-subfield" value={editReaderBaseUrl} onChange={(event) => onEditReaderBaseUrlChange(event.target.value)} placeholder={t('accounts.reader_base_url')} />
                <input className="accounts-edit-subfield" value={editSearchBaseUrl} onChange={(event) => onEditSearchBaseUrlChange(event.target.value)} placeholder={t('accounts.search_base_url')} />
              </>
            ) : (
              <input className="accounts-edit-subfield" value={editBaseUrl} onChange={(event) => onEditBaseUrlChange(event.target.value)} placeholder={t('accounts.base_url_placeholder')} />
            )}
            <input
              className="accounts-edit-subfield"
              type="password"
              value={editApiKey}
              onChange={(event) => onEditApiKeyChange(event.target.value)}
              placeholder={t('accounts.replace_api_key')}
            />
          </div>
        ) : (
          <div className="cell-stack accounts-name-cell">
            <strong>{account.name}</strong>
            <span>{account.id}</span>
          </div>
        )}
      </td>
      <td>
        <StatusBadge tone={toneFromStatus(account.status)}>
          {formatManagedStatus(account.status, t)}
        </StatusBadge>
      </td>
      <td>{account.consecutive_failures}</td>
      <td className="accounts-proxy-cell">
        <BoundProxyTags token={token} accountId={account.id} />
        <div className="account-bind-controls compound-control">
          <CustomSelect
            className="account-bind-select"
            value={bindSelections[account.id] ?? ''}
            onChange={(value) => onBindSelectionChange(account.id, value)}
            ariaLabel={t('table.proxy_binding')}
            options={[
              { value: '', label: t('common.select_proxy') },
              ...proxies.map((proxy) => ({
                value: proxy.id,
                label: proxy.name,
              })),
            ]}
          />
          <button
            type="button"
            className="ghost-button"
            disabled={bindPending}
            onClick={() => onBindAccount(account.id)}
          >
            {t('common.bind')}
          </button>
        </div>
      </td>
      <td>
        <div className="inline-action accounts-actions">
          {isEditing ? (
            <>
              <button
                type="button"
                className="primary-button"
                disabled={updatePending}
                onClick={() => onSaveEdit(account.id)}
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
              onPrimaryClick={() => onStartEdit(account)}
              items={[
                {
                  label: account.enabled ? t('common.disable') : t('common.enable'),
                  disabled: togglePending,
                  onClick: () => onToggleEnabled(account.id, !account.enabled),
                },
                {
                  label: deletePending ? t('common.deleting') : t('common.delete'),
                  danger: true,
                  disabled: deletePending,
                  onClick: () => onDelete(account.id),
                },
              ]}
            />
          )}
        </div>
      </td>
    </tr>
  )
}
