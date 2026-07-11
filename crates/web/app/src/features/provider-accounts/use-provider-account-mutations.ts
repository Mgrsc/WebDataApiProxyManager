import { useMutation, type QueryClient } from '@tanstack/react-query'
import { adminApi } from '../../api'
import type { ProviderId } from '../../types'
import { createAccountName, detectProviderFromApiKey } from './utils'
import type { AccountBulkAction } from './utils'

export function useProviderAccountMutations({
  token,
  t,
  queryClient,
  provider,
  name,
  apiKeyInput,
  baseUrl,
  readerBaseUrl,
  searchBaseUrl,
  batchApiKeys,
  editName,
  editBaseUrl,
  editReaderBaseUrl,
  editSearchBaseUrl,
  editApiKey,
  bindSelections,
  selectedIds,
  bulkProxyId,
  resetCreateForm,
  resetEditForm,
  setSelectedIds,
  setBulkProxyId,
}: {
  token: string
  t: (key: string, values?: Record<string, string | number>) => string
  queryClient: QueryClient
  provider: ProviderId
  name: string
  apiKeyInput: string
  baseUrl: string
  readerBaseUrl: string
  searchBaseUrl: string
  batchApiKeys: string[]
  editName: string
  editBaseUrl: string
  editReaderBaseUrl: string
  editSearchBaseUrl: string
  editApiKey: string
  bindSelections: Record<string, string>
  selectedIds: string[]
  bulkProxyId: string
  resetCreateForm: () => void
  resetEditForm: () => void
  setSelectedIds: (value: string[]) => void
  setBulkProxyId: (value: string) => void
}) {
  const invalidateAccounts = () =>
    queryClient.invalidateQueries({ queryKey: ['provider-accounts'] })

  const createMutation = useMutation({
    mutationFn: async () =>
      adminApi.createProviderAccount(token, {
        provider,
        name,
        api_key: apiKeyInput,
        base_url: baseUrl || undefined,
        reader_base_url: provider === 'jina' ? readerBaseUrl || undefined : undefined,
        search_base_url: provider === 'jina' ? searchBaseUrl || undefined : undefined,
        enabled: true,
      }),
    onSuccess: async () => {
      resetCreateForm()
      await invalidateAccounts()
    },
  })

  const bulkCreateMutation = useMutation({
    mutationFn: async () => {
      if (batchApiKeys.length === 0) {
        throw new Error(t('accounts.batch_empty'))
      }

      const usedNames = new Set<string>()
      const resolvedProviders = new Set(
        batchApiKeys.map((apiKey) => detectProviderFromApiKey(apiKey) ?? provider),
      )
      if (resolvedProviders.size > 1 && (baseUrl || readerBaseUrl || searchBaseUrl)) {
        throw new Error(t('accounts.batch_mixed_base_url'))
      }
      let createdCount = 0
      const failures: string[] = []
      for (const [index, apiKey] of batchApiKeys.entries()) {
        const resolvedProvider = detectProviderFromApiKey(apiKey) ?? provider
        const generatedName = createAccountName(
          resolvedProvider,
          apiKey,
          index,
          usedNames,
        )
        try {
          await adminApi.createProviderAccount(token, {
            provider: resolvedProvider,
            name: generatedName,
            api_key: apiKey,
            base_url: resolvedProvider === 'jina' ? undefined : baseUrl || undefined,
            reader_base_url: resolvedProvider === 'jina' ? readerBaseUrl || undefined : undefined,
            search_base_url: resolvedProvider === 'jina' ? searchBaseUrl || undefined : undefined,
            enabled: true,
          })
          createdCount += 1
        } catch (error) {
          const message = error instanceof Error ? error.message : String(error)
          failures.push(`${generatedName}: ${message}`)
        }
      }

      if (failures.length > 0) {
        throw new Error(
          `${t('accounts.batch_partial_failed')} ${createdCount}/${batchApiKeys.length}. ${failures[0]}`,
        )
      }
    },
    onSuccess: async () => {
      resetCreateForm()
    },
    onSettled: async () => {
      await invalidateAccounts()
    },
  })

  const toggleMutation = useMutation({
    mutationFn: async (payload: { accountId: string; enabled: boolean }) =>
      adminApi.setProviderAccountEnabled(token, payload.accountId, payload.enabled),
    onSuccess: async () => {
      await invalidateAccounts()
    },
  })

  const deleteMutation = useMutation({
    mutationFn: async (accountId: string) =>
      adminApi.deleteProviderAccount(token, accountId),
    onSuccess: async () => {
      await invalidateAccounts()
    },
  })

  const bindMutation = useMutation({
    mutationFn: async (payload: { accountId: string; proxyId: string }) =>
      adminApi.bindProxy(token, payload.accountId, payload.proxyId),
    onSuccess: async (_data, variables) => {
      await Promise.all([
        invalidateAccounts(),
        queryClient.invalidateQueries({ queryKey: ['egress-proxies'] }),
        queryClient.invalidateQueries({
          queryKey: ['bound-proxies', variables.accountId],
        }),
      ])
    },
  })

  const bulkMutation = useMutation({
    mutationFn: async (
      payload:
        | { action: 'enable'; accountIds: string[] }
        | { action: 'disable'; accountIds: string[] }
        | { action: 'delete'; accountIds: string[] }
        | { action: 'bind'; accountIds: string[]; proxyId: string },
    ) => {
      const results = await Promise.allSettled(
        payload.accountIds.map(async (accountId) => {
          switch (payload.action) {
            case 'enable':
              return adminApi.setProviderAccountEnabled(token, accountId, true)
            case 'disable':
              return adminApi.setProviderAccountEnabled(token, accountId, false)
            case 'delete':
              return adminApi.deleteProviderAccount(token, accountId)
            case 'bind':
              return adminApi.bindProxy(token, accountId, payload.proxyId)
          }
        }),
      )
      const failed = results.filter((result) => result.status === 'rejected')
      if (failed.length > 0) {
        const first = failed[0]
        if (first.status === 'rejected') {
          const message =
            first.reason instanceof Error ? first.reason.message : String(first.reason)
          throw new Error(
            `${t('accounts.bulk_partial_failed')} ${payload.accountIds.length - failed.length}/${payload.accountIds.length}. ${message}`,
          )
        }
      }
    },
    onSuccess: async (_data, payload) => {
      setSelectedIds([])
      if (payload.action === 'bind') {
        setBulkProxyId('')
        await Promise.all([
          invalidateAccounts(),
          queryClient.invalidateQueries({ queryKey: ['egress-proxies'] }),
          ...payload.accountIds.map((accountId) =>
            queryClient.invalidateQueries({ queryKey: ['bound-proxies', accountId] }),
          ),
        ])
        return
      }
      await invalidateAccounts()
    },
  })

  const updateMutation = useMutation({
    mutationFn: async (accountId: string) => {
      const payload: {
        name?: string
        base_url?: string
        clear_base_url?: boolean
        api_key?: string
        reader_base_url?: string
        clear_reader_base_url?: boolean
        search_base_url?: string
        clear_search_base_url?: boolean
      } = {}
      if (editName.trim()) {
        payload.name = editName.trim()
      }
      if (editApiKey.trim()) {
        payload.api_key = editApiKey.trim()
      }
      if (editBaseUrl.trim()) {
        payload.base_url = editBaseUrl.trim()
      } else {
        payload.clear_base_url = true
      }
      if (editReaderBaseUrl.trim()) payload.reader_base_url = editReaderBaseUrl.trim()
      else payload.clear_reader_base_url = true
      if (editSearchBaseUrl.trim()) payload.search_base_url = editSearchBaseUrl.trim()
      else payload.clear_search_base_url = true
      return adminApi.updateProviderAccount(token, accountId, payload)
    },
    onSuccess: async () => {
      resetEditForm()
      await invalidateAccounts()
    },
  })

  const runBulkAction = (action: AccountBulkAction) => {
    if (selectedIds.length === 0) {
      return
    }
    if (action === 'bind') {
      if (!bulkProxyId) {
        return
      }
      void bulkMutation.mutateAsync({
        action,
        accountIds: selectedIds,
        proxyId: bulkProxyId,
      })
      return
    }
    if (action === 'delete' && !window.confirm(t('accounts.bulk_delete_confirm'))) {
      return
    }
    void bulkMutation.mutateAsync({ action, accountIds: selectedIds })
  }

  const handleBindAccount = (accountId: string) => {
    const proxyId = bindSelections[accountId]
    if (proxyId) {
      void bindMutation.mutateAsync({ accountId, proxyId })
    }
  }

  return {
    createMutation,
    bulkCreateMutation,
    toggleMutation,
    deleteMutation,
    bindMutation,
    bulkMutation,
    updateMutation,
    runBulkAction,
    handleBindAccount,
    onSaveEdit: (accountId: string) => void updateMutation.mutateAsync(accountId),
    onToggleEnabled: (accountId: string, enabled: boolean) =>
      void toggleMutation.mutateAsync({ accountId, enabled }),
    onDelete: (accountId: string) => void deleteMutation.mutateAsync(accountId),
    onCreate: () => void createMutation.mutateAsync(),
    onBulkCreate: () => void bulkCreateMutation.mutateAsync(),
  }
}
