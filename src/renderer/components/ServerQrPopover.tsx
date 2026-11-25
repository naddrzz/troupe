import type { ServerSetupAnswers } from '@shared/types/server-setup.types'
import { useEffect, useState } from 'react'
import { QrCode } from '@/components/icons'
import { ServerQrPanel } from '@/components/ServerQrPanel'
import { ToolStrip } from '@/components/shell/tool-strip/ToolStrip'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { useIsStandaloneServer } from '@/hooks/use-is-standalone-server'
import { useServerAdmitsRemoteWrites } from '@/hooks/use-server-admits-remote-writes'
import { logFrontendError } from '@/lib/log-api'
import { isTauriContext } from '@/lib/tauri-runtime'
import { webSetupApi } from '@/lib/web-setup-api'

/**
 * Tool strip popover for standalone `troupe-server` web clients: displays a
 * scannable QR code to open the same server on a phone or tablet.
 *
 * Explicitly gated off on Tauri desktop (`isTauriContext()` — desktop owns
 * `RemoteAccessPopover` instead) and on desktop shared-live remote hosts
 * (`!useIsStandaloneServer()`). Lazy-fetches `GET /server/config` on first
 * open so it adds zero boot traffic.
 */
export function ServerQrPopover(): React.JSX.Element | null {
  const isStandalone = useIsStandaloneServer()
  if (isTauriContext() || !isStandalone) return null
  return <ServerQrPopoverInner />
}

function ServerQrPopoverInner(): React.JSX.Element {
  const [open, setOpen] = useState(false)
  const [config, setConfig] = useState<ServerSetupAnswers | null>(null)
  const [lanHost, setLanHost] = useState<string | undefined>(undefined)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [attempt, setAttempt] = useState(0)
  const admitsWrites = useServerAdmitsRemoteWrites()

  // biome-ignore lint/correctness/useExhaustiveDependencies: attempt intentionally re-runs the load (Try again button)
  useEffect(() => {
    if (!open) return
    let cancelled = false
    setLoading(true)
    setError(null)
    void webSetupApi
      .getServerConfig()
      .then((res) => {
        if (cancelled) return
        if (res.success) {
          setConfig(res.data.config)
          setLanHost(res.data.lanHost)
          setLoading(false)
        } else {
          void logFrontendError({
            level: 'warn',
            source: 'ServerQrPopover',
            message: `GET /server/config failed (code ${res.code})`
          })
          setError('Could not load the server address.')
          setLoading(false)
        }
      })
      .catch((err) => {
        if (cancelled) return
        void logFrontendError({
          level: 'warn',
          source: 'ServerQrPopover',
          message: `GET /server/config threw: ${err instanceof Error ? err.message : String(err)}`
        })
        setError('Could not load the server address.')
        setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [open, attempt])

  const allowWrites = config ? config.allowRemoteWrites : admitsWrites

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <ToolStrip.Button
          label="Connect device"
          aria-label="Show server QR code"
          aria-pressed={open}
          className="data-[state=open]:bg-foreground/[0.06] data-[state=open]:text-foreground"
          icon={<QrCode size={14} aria-hidden />}
        />
      </PopoverTrigger>
      <PopoverContent side="left" align="end" className="w-72 p-4">
        <div className="space-y-3">
          <div>
            <h4 className="font-medium text-sm text-foreground">Connect another device</h4>
            <p className="text-xs text-muted-foreground mt-1">
              Scan the QR code to open this Troupe server on a phone or tablet.
            </p>
          </div>
          <ServerQrPanel
            config={config}
            lanHost={lanHost}
            loading={loading}
            error={error}
            onRetry={() => setAttempt((n) => n + 1)}
            allowWrites={allowWrites}
          />
        </div>
      </PopoverContent>
    </Popover>
  )
}
