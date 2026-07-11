import { ErrorBanner, Spinner } from '../../ui/shared'
import { AccountsPanel } from './accounts-table'
import { CreateAccountDrawer } from './create-account-drawer'
import { useProviderAccountsPage } from './use-provider-accounts'

export function ProviderAccountsPage() {
  const state = useProviderAccountsPage()

  if (state.accountsQuery.isLoading) return <Spinner />
  if (state.accountsQuery.error) return <ErrorBanner message={state.accountsQuery.error.message} />

  return (
    <div className="page-grid">
      <section className="hero-strip">
        <div>
          <span className="eyebrow">{state.t('accounts.eyebrow')}</span>
          <h2>{state.t('accounts.title')}</h2>
        </div>
        <div className="hero-actions">
          <button
            type="button"
            className="primary-button"
            onClick={() => state.setDrawerOpen(true)}
          >
            + {state.t('common.add_account')}
          </button>
        </div>
      </section>

      <CreateAccountDrawer
        open={state.drawerOpen}
        onClose={() => state.setDrawerOpen(false)}
        provider={state.provider}
        providerAutoDetected={state.providerAutoDetected}
        detectedProvider={state.detectedProvider}
        name={state.name}
        apiKeyInput={state.apiKeyInput}
        batchApiKeysInput={state.batchApiKeysInput}
        baseUrl={state.baseUrl}
        readerBaseUrl={state.readerBaseUrl}
        searchBaseUrl={state.searchBaseUrl}
        batchApiKeysLength={state.batchApiKeys.length}
        detectedBatchCount={state.detectedBatchCount}
        unknownBatchCount={state.unknownBatchCount}
        createPending={state.createMutation.isPending}
        bulkPending={state.bulkCreateMutation.isPending}
        createError={state.createMutation.error?.message}
        bulkError={state.bulkCreateMutation.error?.message}
        onProviderChange={state.setProvider}
        onNameChange={state.setName}
        onApiKeyChange={state.handleApiKeyChange}
        onBatchApiKeysChange={state.setBatchApiKeysInput}
        onBaseUrlChange={state.setBaseUrl}
        onReaderBaseUrlChange={state.setReaderBaseUrl}
        onSearchBaseUrlChange={state.setSearchBaseUrl}
        onCreate={state.onCreate}
        onBulkCreate={state.onBulkCreate}
        t={state.t}
      />

      <AccountsPanel
        token={state.token}
        title={state.t('accounts.pool')}
        description={state.t('accounts.count', {
          count: state.accountsQuery.data?.length ?? 0,
        })}
        providerFilter={state.providerFilter}
        selectedIds={state.selectedIds}
        sortMode={state.sortMode}
        visibleAccounts={state.visibleAccounts}
        accounts={state.accountsQuery.data ?? []}
        proxies={state.proxiesQuery.data ?? []}
        bindSelections={state.bindSelections}
        bulkProxyId={state.bulkProxyId}
        allVisibleSelected={state.allVisibleSelected}
        editingId={state.editingId}
        editName={state.editName}
        editBaseUrl={state.editBaseUrl}
        editReaderBaseUrl={state.editReaderBaseUrl}
        editSearchBaseUrl={state.editSearchBaseUrl}
        editApiKey={state.editApiKey}
        bindPending={state.bindMutation.isPending}
        bulkPending={state.bulkMutation.isPending}
        updatePending={state.updateMutation.isPending}
        togglePending={state.toggleMutation.isPending}
        deletePending={state.deleteMutation.isPending}
        updateError={state.updateMutation.error?.message}
        bindError={state.bindMutation.error?.message}
        bulkError={state.bulkMutation.error?.message}
        onProviderFilterChange={state.handleProviderFilterChange}
        onToggleSort={() =>
          state.setSortMode((current) =>
            current === 'default' ? 'failures_desc' : 'default',
          )
        }
        onToggleSelectAllVisible={state.toggleSelectAllVisible}
        onToggleSelection={state.toggleSelection}
        onBulkProxyChange={state.setBulkProxyId}
        onRunBulkAction={state.runBulkAction}
        onEditNameChange={state.setEditName}
        onEditBaseUrlChange={state.setEditBaseUrl}
        onEditReaderBaseUrlChange={state.setEditReaderBaseUrl}
        onEditSearchBaseUrlChange={state.setEditSearchBaseUrl}
        onEditApiKeyChange={state.setEditApiKey}
        onBindSelectionChange={state.handleBindSelectionChange}
        onBindAccount={state.handleBindAccount}
        onSaveEdit={state.onSaveEdit}
        onCancelEdit={state.onCancelEdit}
        onStartEdit={state.handleStartEdit}
        onToggleEnabled={state.onToggleEnabled}
        onDelete={state.onDelete}
        t={state.t}
      />
    </div>
  )
}
