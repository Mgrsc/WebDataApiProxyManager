import { useQuery, useQueryClient } from '@tanstack/react-query'
import { adminApi } from '../../api'
import { useSession } from '../../app'
import { useLocale } from '../../i18n'
import { useProviderAccountMutations } from './use-provider-account-mutations'
import { useProviderAccountsState } from './use-provider-accounts-state'

export function useProviderAccountsPage() {
  const { token } = useSession()
  const { t } = useLocale()
  const queryClient = useQueryClient()

  const accountsQuery = useQuery({
    queryKey: ['provider-accounts', token],
    queryFn: () => adminApi.listProviderAccounts(token),
  })
  const proxiesQuery = useQuery({
    queryKey: ['egress-proxies', token],
    queryFn: () => adminApi.listEgressProxies(token),
  })

  const state = useProviderAccountsState(accountsQuery.data)
  const mutations = useProviderAccountMutations({
    token,
    t,
    queryClient,
    provider: state.provider,
    name: state.name,
    apiKeyInput: state.apiKeyInput,
    baseUrl: state.baseUrl,
    readerBaseUrl: state.readerBaseUrl,
    searchBaseUrl: state.searchBaseUrl,
    batchApiKeys: state.batchApiKeys,
    editName: state.editName,
    editBaseUrl: state.editBaseUrl,
    editReaderBaseUrl: state.editReaderBaseUrl,
    editSearchBaseUrl: state.editSearchBaseUrl,
    editApiKey: state.editApiKey,
    bindSelections: state.bindSelections,
    selectedIds: state.selectedIds,
    bulkProxyId: state.bulkProxyId,
    resetCreateForm: state.resetCreateForm,
    resetEditForm: state.resetEditForm,
    setSelectedIds: state.setSelectedIds,
    setBulkProxyId: state.setBulkProxyId,
  })

  return {
    token,
    t,
    accountsQuery,
    proxiesQuery,
    ...state,
    ...mutations,
    onCancelEdit: state.resetEditForm,
  }
}
