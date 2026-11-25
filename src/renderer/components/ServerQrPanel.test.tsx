import type { ServerSetupAnswers } from '@shared/types/server-setup.types'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ServerQrPanel } from './ServerQrPanel'

vi.mock('qrcode.react', () => ({
  QRCodeSVG: ({ value }: { value: string }) => <div data-testid="server-qr" data-value={value} />
}))

const { mockToken, mockLog } = vi.hoisted(() => ({
  mockToken: vi.fn((): string | null => null),
  mockLog: vi.fn(() => Promise.resolve())
}))

vi.mock('@/lib/web-auth-token', () => ({
  getWebAuthToken: () => mockToken()
}))

vi.mock('@/lib/log-api', () => ({
  logFrontendError: (...args: unknown[]) => mockLog(...args)
}))

const PUBLIC_CONFIG: ServerSetupAnswers = {
  host: '0.0.0.0',
  port: 8080,
  sessionsDir: '/state/sessions',
  projectsFile: '/state/projects.json',
  allowRemoteWrites: true,
  updateChannel: null,
  updateIntervalSecs: 21600
}

const LOOPBACK_CONFIG: ServerSetupAnswers = {
  ...PUBLIC_CONFIG,
  host: '127.0.0.1',
  allowRemoteWrites: false
}

describe('ServerQrPanel', () => {
  const writeTextMock = vi.fn(() => Promise.resolve())

  beforeEach(() => {
    vi.clearAllMocks()
    mockToken.mockReturnValue(null)
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: writeTextMock }
    })
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('renders a QR with detected lanHost and #token= without printing the token in visible text', () => {
    mockToken.mockReturnValue('secret-tok-99')
    render(<ServerQrPanel config={PUBLIC_CONFIG} lanHost="192.168.1.42" allowWrites={true} />)

    const qr = screen.getByTestId('server-qr')
    expect(qr.getAttribute('data-value')).toBe('http://192.168.1.42:8080/#token=secret-tok-99')
    expect(screen.getByText('http://192.168.1.42:8080')).toBeInTheDocument()
    expect(screen.getByText(/prompt agents, run commands, and edit files/)).toBeInTheDocument()
    expect(screen.queryByText(/secret-tok-99/)).toBeNull()
  })

  it('renders view-only warning when allowWrites is false and copies full share URL', async () => {
    mockToken.mockReturnValue('secret-view-tok')
    render(<ServerQrPanel config={PUBLIC_CONFIG} lanHost="10.0.0.5" allowWrites={false} />)

    expect(screen.getByText(/view sessions on this server/)).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Copy server link' }))

    await waitFor(() =>
      expect(writeTextMock).toHaveBeenCalledWith('http://10.0.0.5:8080/#token=secret-view-tok')
    )
    expect(mockLog).toHaveBeenCalledWith(
      expect.objectContaining({
        level: 'info',
        source: 'ServerQrPanel',
        message: 'copied server connect link (hasToken=true)'
      })
    )
  })

  it('shows loopback-only explanation instead of an unscannable localhost QR', () => {
    render(<ServerQrPanel config={LOOPBACK_CONFIG} allowWrites={false} />)

    expect(screen.queryByTestId('server-qr')).toBeNull()
    expect(screen.getByText(/bound to localhost only/i)).toBeInTheDocument()
  })

  it('shows loading spinner and retryable error states', () => {
    const { rerender } = render(<ServerQrPanel config={null} loading allowWrites={false} />)
    expect(screen.getByLabelText('Loading server address')).toBeInTheDocument()

    const onRetry = vi.fn()
    rerender(
      <ServerQrPanel config={null} error="Could not load." onRetry={onRetry} allowWrites={false} />
    )
    expect(screen.getByText('Could not load.')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }))
    expect(onRetry).toHaveBeenCalledTimes(1)
  })
})
