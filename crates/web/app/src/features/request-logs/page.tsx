import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { adminApi } from '../../api'
import { useSession } from '../../app'
import { useLocale } from '../../i18n'
import { ErrorBanner, Spinner, useCopyFeedback } from '../../ui/shared'
import { RequestLogsFiltersPanel } from './filters-panel'
import { RequestLogsTable } from './logs-table'
import type { RequestLogFilters } from './types'
import { buildRequestLogFilters } from './utils'

export function RequestLogsPage() {
  const { token } = useSession()
  const { t } = useLocale()
  const { copiedId, copy } = useCopyFeedback(1600)

  const [provider, setProvider] = useState('')
  const [statusRange, setStatusRange] = useState('')
  const [since, setSince] = useState('')
  const [until, setUntil] = useState('')
  const [advancedOpen, setAdvancedOpen] = useState(false)
  const [latencyMin, setLatencyMin] = useState('')
  const [latencyMax, setLatencyMax] = useState('')
  const [apiKeyName, setApiKeyName] = useState('')
  const [limit, setLimit] = useState('100')
  const [appliedFilters, setAppliedFilters] = useState<RequestLogFilters>({ limit: 100 })
  const [refreshVersion, setRefreshVersion] = useState(0)

  const logsQuery = useQuery({
    queryKey: ['request-logs', token, appliedFilters, refreshVersion],
    queryFn: () => adminApi.listRequestLogs(token, appliedFilters),
  })

  const applyFilters = () => {
    setAppliedFilters(
      buildRequestLogFilters({
        provider,
        statusRange,
        latencyMin,
        latencyMax,
        since,
        until,
        apiKeyName,
        limit,
      }),
    )
    setRefreshVersion((value) => value + 1)
  }

  if (logsQuery.isLoading) return <Spinner />
  if (logsQuery.error) return <ErrorBanner message={logsQuery.error.message} />

  return (
    <div className="page-grid">
      <section className="hero-strip">
        <div>
          <span className="eyebrow">{t('requests.title')}</span>
          <h2>{t('requests.title')}</h2>
        </div>
      </section>

      <RequestLogsFiltersPanel
        title={t('requests.title')}
        description={t('requests.desc')}
        provider={provider}
        statusRange={statusRange}
        since={since}
        until={until}
        advancedOpen={advancedOpen}
        latencyMin={latencyMin}
        latencyMax={latencyMax}
        apiKeyName={apiKeyName}
        limit={limit}
        onProviderChange={setProvider}
        onStatusRangeChange={setStatusRange}
        onSinceChange={setSince}
        onUntilChange={setUntil}
        onToggleAdvanced={() => setAdvancedOpen((open) => !open)}
        onLatencyMinChange={setLatencyMin}
        onLatencyMaxChange={setLatencyMax}
        onApiKeyNameChange={setApiKeyName}
        onLimitChange={setLimit}
        onApply={applyFilters}
        t={t}
      >
        <RequestLogsTable
          logs={logsQuery.data ?? []}
          copiedId={copiedId}
          onCopy={(value, id) => void copy(value, id)}
          t={t}
        />
      </RequestLogsFiltersPanel>
    </div>
  )
}
