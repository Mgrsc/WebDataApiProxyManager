import { Drawer, ErrorBanner } from '../../ui/shared'

export function CreateProxyDrawer({
  open,
  onClose,
  name,
  proxyUrl,
  createPending,
  createError,
  onNameChange,
  onProxyUrlChange,
  onCreate,
  t,
}: {
  open: boolean
  onClose: () => void
  name: string
  proxyUrl: string
  createPending: boolean
  createError?: string
  onNameChange: (value: string) => void
  onProxyUrlChange: (value: string) => void
  onCreate: () => void
  t: (key: string, values?: Record<string, string | number>) => string
}) {
  return (
    <Drawer open={open} onClose={onClose} title={t('proxies.create')}>
      <p className="panel-copy">{t('proxies.create_desc')}</p>
      <div className="stack-form stack-form-spaced">
        <label className="field">
          <span>{t('table.name')}</span>
          <input
            value={name}
            onChange={(event) => onNameChange(event.target.value)}
            placeholder="e.g. us-west-01"
          />
        </label>
        <label className="field">
          <span>{t('proxies.proxy_url')}</span>
          <input
            value={proxyUrl}
            onChange={(event) => onProxyUrlChange(event.target.value)}
            placeholder="socks5://127.0.0.1:1080"
          />
        </label>
        {createError ? <ErrorBanner message={createError} /> : null}
        <button
          type="button"
          className="primary-button"
          disabled={createPending || !name.trim() || !proxyUrl.trim()}
          onClick={onCreate}
        >
          {createPending ? t('common.creating') : t('proxies.add')}
        </button>
      </div>
    </Drawer>
  )
}
