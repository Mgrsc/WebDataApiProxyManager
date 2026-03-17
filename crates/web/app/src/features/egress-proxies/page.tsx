import { useQuery, useQueryClient } from '@tanstack/react-query'
import { adminApi } from '../../api'
import { useSession } from '../../app'
import { useLocale } from '../../i18n'
import { ErrorBanner, Spinner } from '../../ui/shared'
import { CreateProxyDrawer } from './create-proxy-drawer'
import { useEgressProxiesStateAndMutations } from './hooks'
import { ProxiesTable } from './proxies-table'

export function EgressProxiesPage() {
  const { token } = useSession()
  const { t } = useLocale()
  const queryClient = useQueryClient()

  const proxiesQuery = useQuery({
    queryKey: ['egress-proxies', token],
    queryFn: () => adminApi.listEgressProxies(token),
  })
  const state = useEgressProxiesStateAndMutations({ token, queryClient })

  if (proxiesQuery.isLoading) return <Spinner />
  if (proxiesQuery.error) return <ErrorBanner message={proxiesQuery.error.message} />

  return (
    <div className="page-grid">
      <section className="hero-strip">
        <div>
          <span className="eyebrow">{t('proxies.eyebrow')}</span>
          <h2>{t('proxies.title')}</h2>
        </div>
        <div className="hero-actions">
          <button
            type="button"
            className="primary-button"
            onClick={() => state.setDrawerOpen(true)}
          >
            + {t('proxies.add')}
          </button>
        </div>
      </section>

      <CreateProxyDrawer
        open={state.drawerOpen}
        onClose={() => state.setDrawerOpen(false)}
        name={state.name}
        proxyUrl={state.proxyUrl}
        createPending={state.createMutation.isPending}
        createError={state.createMutation.error?.message}
        onNameChange={state.setName}
        onProxyUrlChange={state.setProxyUrl}
        onCreate={state.onCreate}
        t={t}
      />

      <ProxiesTable
        title={t('proxies.pool')}
        description={t('proxies.count', { count: proxiesQuery.data?.length ?? 0 })}
        proxies={proxiesQuery.data ?? []}
        editingId={state.editingId}
        editName={state.editName}
        editProxyUrl={state.editProxyUrl}
        updatePending={state.updateMutation.isPending}
        togglePending={state.toggleMutation.isPending}
        testingId={state.testingId}
        testResults={state.testResults}
        updateError={state.updateMutation.error?.message}
        toggleError={state.toggleMutation.error?.message}
        testError={state.testMutation.error?.message}
        onEditNameChange={state.setEditName}
        onEditProxyUrlChange={state.setEditProxyUrl}
        onSaveEdit={state.onSaveEdit}
        onCancelEdit={state.resetEditForm}
        onStartEdit={state.handleStartEdit}
        onToggleEnabled={state.onToggleEnabled}
        onTestProxy={state.onTestProxy}
        t={t}
      />
    </div>
  )
}
