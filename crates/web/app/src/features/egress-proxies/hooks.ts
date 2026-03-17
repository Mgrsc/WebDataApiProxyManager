import { useMutation, type QueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { adminApi } from '../../api'
import type { EgressProxySummary, EgressProxyTestResult } from '../../types'

export function useEgressProxiesStateAndMutations({
  token,
  queryClient,
}: {
  token: string
  queryClient: QueryClient
}) {
  const [drawerOpen, setDrawerOpen] = useState(false)
  const [name, setName] = useState('')
  const [proxyUrl, setProxyUrl] = useState('')
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editName, setEditName] = useState('')
  const [editProxyUrl, setEditProxyUrl] = useState('')
  const [testingId, setTestingId] = useState<string | null>(null)
  const [testResults, setTestResults] = useState<Record<string, EgressProxyTestResult>>({})

  const resetCreateForm = () => {
    setName('')
    setProxyUrl('')
    setDrawerOpen(false)
  }

  const resetEditForm = () => {
    setEditingId(null)
    setEditName('')
    setEditProxyUrl('')
  }

  const createMutation = useMutation({
    mutationFn: async () =>
      adminApi.createEgressProxy(token, {
        name,
        proxy_url: proxyUrl,
        enabled: true,
      }),
    onSuccess: async () => {
      resetCreateForm()
      await queryClient.invalidateQueries({ queryKey: ['egress-proxies'] })
    },
  })

  const updateMutation = useMutation({
    mutationFn: async (proxyId: string) => {
      const payload: {
        name?: string
        proxy_url?: string
      } = {}
      if (editName.trim()) {
        payload.name = editName.trim()
      }
      if (editProxyUrl.trim()) {
        payload.proxy_url = editProxyUrl.trim()
      }
      return adminApi.updateEgressProxy(token, proxyId, payload)
    },
    onSuccess: async () => {
      resetEditForm()
      await queryClient.invalidateQueries({ queryKey: ['egress-proxies'] })
    },
  })

  const toggleMutation = useMutation({
    mutationFn: async (payload: { proxyId: string; enabled: boolean }) =>
      adminApi.updateEgressProxy(token, payload.proxyId, { enabled: payload.enabled }),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ['egress-proxies'] })
    },
  })

  const testMutation = useMutation({
    mutationFn: async (proxyId: string) => adminApi.testEgressProxy(token, proxyId),
    onMutate: async (proxyId: string) => {
      setTestingId(proxyId)
    },
    onSuccess: async (result) => {
      setTestResults((current) => ({ ...current, [result.proxy_id]: result }))
    },
    onSettled: async () => {
      setTestingId(null)
    },
  })

  const handleStartEdit = (proxy: EgressProxySummary) => {
    setEditingId(proxy.id)
    setEditName(proxy.name)
    setEditProxyUrl(proxy.proxy_url)
  }

  return {
    drawerOpen,
    name,
    proxyUrl,
    editingId,
    editName,
    editProxyUrl,
    testingId,
    testResults,
    createMutation,
    updateMutation,
    toggleMutation,
    testMutation,
    setDrawerOpen,
    setName,
    setProxyUrl,
    setEditName,
    setEditProxyUrl,
    resetEditForm,
    handleStartEdit,
    onCreate: () => void createMutation.mutateAsync(),
    onSaveEdit: (proxyId: string) => void updateMutation.mutateAsync(proxyId),
    onToggleEnabled: (proxyId: string, enabled: boolean) =>
      void toggleMutation.mutateAsync({ proxyId, enabled }),
    onTestProxy: (proxyId: string) => void testMutation.mutateAsync(proxyId),
  }
}
