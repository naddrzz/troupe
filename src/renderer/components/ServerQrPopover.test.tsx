import type { ServerSetupAnswers } from '@shared/types/server-setup.types'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { TooltipProvider } from '@/components/ui/tooltip'

const { tauriRef, standaloneRef, getServerConfigMock, mockToken } = vi.hoisted(() => ({
  tauriRef: { current: false },
  standaloneRef: { current: true },
  getServerConfigMock: vi.fn(),
  mockToken: vi.fn((): string | null => 'tok-abc')
}))

vi.mock('@/lib/tauri-runtime', () => ({
  isTauriContext: () => tauriRef.current
}))
vi.mock('@/hooks/use-is-standalone-server', () => ({
  useIsStandaloneServer: () => !tauriRef.current && standaloneRef.current
}))
vi.mock('@/hooks/use-server-admits-remote-writes', () => ({
  useServerAdmitsRemoteWrites: () => true
}))
vi.mock('@/lib/web-setup-api', () => ({
  webSetupApi: {
    getServerConfig: () => getServerConfigMock()
  }
}))
vi.mock('@/lib/web-auth-token', () => ({
  getWebAuthToken: () => mockToken()
}))
vi.mock('@/lib/log-api', () => ({
  logFrontendError: vi.fn(() => Promise.resolve())
}))
vi.mock('qrcode.react', () => ({
  QRCodeSVG: ({ value }: { value: string }) => <div data-testid="server-qr" data-value={value} />
}))

import { ServerQrPopover } from './ServerQrPopover'

const CONFIG: ServerSetupAnswers = {
  host: '0.0.0.0',
  port: 8080,
  sessionsDir: '/state/sessions',
  projectsFile: '/state/projects.json',
  allowRemoteWrites: true,
  updateChannel: null,
  updateIntervalSecs: 21600
}

function renderPopover(): ReturnType<typeof render> {
  return render(
    <TooltipProvider>
      <ServerQrPopover />
    </TooltipProvider>
  )
}

describe('ServerQrPopover', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    tauriRef.current = false
    standaloneRef.current = true
    mockToken.mockReturnValue('tok-abc')
    getServerConfigMock.mockResolvedValue({
      success: true,
      data: { config: CONFIG, lanHost: '192.168.1.42' }
    })
  })

  it('renders nothing on Tauri desktop (explicitly gated)', () => {
    tauriRef.current = true
    const { container } = renderPopover()
    expect(container).toBeEmptyDOMElement()
    expect(getServerConfigMock).not.toHaveBeenCalled()
  })

  it('renders nothing on desktop shared-live remote (non-standalone web)', () => {
    standaloneRef.current = false
    const { container } = renderPopover()
    expect(container).toBeEmptyDOMElement()
    expect(getServerConfigMock).not.toHaveBeenCalled()
  })

  it('lazy-fetches /server/config on open and renders the QR code', async () => {
    renderPopover()
    expect(getServerConfigMock).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('button', { name: 'Show server QR code' }))

    await waitFor(() => expect(screen.getByTestId('server-qr')).toBeInTheDocument())
    expect(getServerConfigMock).toHaveBeenCalledTimes(1)
    expect(screen.getByTestId('server-qr').getAttribute('data-value')).toBe(
      'http://192.168.1.42:8080/#token=tok-abc'
    )
  })
})
