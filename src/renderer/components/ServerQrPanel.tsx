import type { ServerSetupAnswers } from '@shared/types/server-setup.types'
import { QRCodeSVG } from 'qrcode.react'
import { useMemo, useState } from 'react'
import { AlertCircle, Check, Copy, ShieldAlert } from '@/components/icons'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'
import { logFrontendError } from '@/lib/log-api'
import { buildServerShareUrl } from '@/lib/server-qr-url'
import { getWebAuthToken } from '@/lib/web-auth-token'

export interface ServerQrPanelProps {
  /** Effective server config from `GET /server/config`. */
  config: ServerSetupAnswers | null
  /** Detected LAN IPv4 address returned by `GET /server/config` on `0.0.0.0` binds. */
  lanHost?: string
  /** Whether `/server/config` is currently loading. */
  loading?: boolean
  /** Error message when `/server/config` failed to load. */
  error?: string | null
  /** Retry callback for the error state. */
  onRetry?: () => void
  /** Whether remote clients connected to this server have write access. */
  allowWrites: boolean
}

/**
 * Shared QR panel for standalone `troupe-server` — mounted inside both
 * `ServerQrPopover` (tool strip) and `ServerSection` (Settings → Server,
 * reachable on desktop-width and phone web shells).
 *
 * Security posture mirrors `RemoteAccessPopover`: the `#token=` fragment rides
 * inside the QR and the "Copy link" clipboard action, never in visible DOM
 * text or logs, paired with a trust warning naming what the link grants.
 */
export function ServerQrPanel({
  config,
  lanHost,
  loading = false,
  error = null,
  onRetry,
  allowWrites
}: ServerQrPanelProps): React.JSX.Element {
  const [copiedUrl, setCopiedUrl] = useState(false)

  const shareResult = useMemo(() => {
    const origin = typeof window !== 'undefined' && window.location ? window.location.origin : ''
    const hostname =
      typeof window !== 'undefined' && window.location ? window.location.hostname : ''
    return buildServerShareUrl({
      origin,
      hostname,
      config,
      lanHost,
      token: getWebAuthToken()
    })
  }, [config, lanHost])

  if (loading) {
    return (
      <div className="flex items-center justify-center py-6">
        <Spinner label="Loading server address" />
      </div>
    )
  }

  if (error) {
    return (
      <div className="space-y-2">
        <div className="flex items-start gap-2 text-sm text-destructive bg-destructive/10 border border-destructive/30 rounded-md px-3 py-2">
          <AlertCircle className="w-4 h-4 mt-0.5 shrink-0" />
          <span className="min-w-0 [overflow-wrap:anywhere]">{error}</span>
        </div>
        {onRetry && (
          <Button type="button" size="xs" variant="outline" onClick={onRetry}>
            Try again
          </Button>
        )}
      </div>
    )
  }

  if (shareResult.status !== 'ready') {
    return (
      <div className="flex items-start gap-2 text-xs text-muted-foreground bg-muted/50 border border-border rounded-md px-3 py-2.5">
        <AlertCircle className="w-4 h-4 mt-0.5 shrink-0 text-muted-foreground" />
        <span className="min-w-0 [overflow-wrap:anywhere]">{shareResult.reason}</span>
      </div>
    )
  }

  const handleCopyLink = async (): Promise<void> => {
    try {
      await navigator.clipboard.writeText(shareResult.shareUrl)
      setCopiedUrl(true)
      setTimeout(() => setCopiedUrl(false), 1500)
      void logFrontendError({
        level: 'info',
        source: 'ServerQrPanel',
        message: `copied server connect link (hasToken=${shareResult.hasToken})`
      })
    } catch {
      // Clipboard unavailable; ignore.
    }
  }

  return (
    <div className="space-y-2.5">
      {/* White pad so the black QR modules are legible in dark themes. */}
      <div className="flex justify-center">
        <div className="rounded-lg bg-primary-foreground p-2">
          <QRCodeSVG value={shareResult.shareUrl} size={160} level="M" />
        </div>
      </div>
      <p className="text-center font-mono text-xs text-muted-foreground break-all">
        {shareResult.displayOrigin}
      </p>
      {shareResult.hasToken && (
        <div className="flex items-start gap-2 text-xs text-warning bg-warning/10 border border-warning/30 rounded-md px-3 py-2">
          <ShieldAlert className="w-4 h-4 mt-0.5 shrink-0" />
          <span>
            {allowWrites
              ? 'This link carries your access token (after the #). Anyone who has it can prompt agents, run commands, and edit files on this server. Share it only with devices you trust.'
              : 'This link carries your access token (after the #). Anyone who has it can view sessions on this server. Share it only with devices you trust.'}
          </span>
        </div>
      )}
      <Button
        type="button"
        variant="secondary"
        size="xs"
        onClick={() => void handleCopyLink()}
        className="w-full min-h-8 max-sm:min-h-11 [&_svg]:size-3.5"
        aria-label="Copy server link"
      >
        {copiedUrl ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
        {copiedUrl ? 'Copied' : 'Copy link'}
      </Button>
    </div>
  )
}
