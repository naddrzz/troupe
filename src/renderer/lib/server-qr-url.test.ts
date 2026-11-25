import type { ServerSetupAnswers } from '@shared/types/server-setup.types'
import { describe, expect, it } from 'vitest'
import { buildServerShareUrl } from './server-qr-url'

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

describe('buildServerShareUrl', () => {
  it('uses the current non-loopback HTTPS origin and appends #token= when present', () => {
    const result = buildServerShareUrl({
      origin: 'https://troupe.example.com',
      hostname: 'troupe.example.com',
      config: PUBLIC_CONFIG,
      lanHost: '192.168.1.42',
      token: 'abc/123+xyz'
    })

    expect(result).toEqual({
      status: 'ready',
      displayOrigin: 'https://troupe.example.com',
      shareUrl: 'https://troupe.example.com/#token=abc%2F123%2Bxyz',
      hasToken: true,
      isPlaintextRemote: false
    })
  })

  it('uses the current LAN HTTP origin and marks isPlaintextRemote true', () => {
    const result = buildServerShareUrl({
      origin: 'http://192.168.1.42:8080',
      hostname: '192.168.1.42',
      config: PUBLIC_CONFIG,
      token: null
    })

    expect(result).toEqual({
      status: 'ready',
      displayOrigin: 'http://192.168.1.42:8080',
      shareUrl: 'http://192.168.1.42:8080/',
      hasToken: false,
      isPlaintextRemote: true
    })
  })

  it('substitutes detected lanHost when browser is on localhost and server binds 0.0.0.0', () => {
    const result = buildServerShareUrl({
      origin: 'http://localhost:9090',
      hostname: 'localhost',
      config: { ...PUBLIC_CONFIG, port: 9090 },
      lanHost: '192.168.1.50',
      token: 'tok-1'
    })

    expect(result).toEqual({
      status: 'ready',
      displayOrigin: 'http://192.168.1.50:9090',
      shareUrl: 'http://192.168.1.50:9090/#token=tok-1',
      hasToken: true,
      isPlaintextRemote: true
    })
  })

  it('returns loopback-only when browser is on localhost and server binds 127.0.0.1', () => {
    const result = buildServerShareUrl({
      origin: 'http://127.0.0.1:8080',
      hostname: '127.0.0.1',
      config: LOOPBACK_CONFIG,
      token: null
    })

    expect(result.status).toBe('loopback-only')
  })

  it('returns no-lan-address when on localhost, bound 0.0.0.0, and lanHost is missing', () => {
    const result = buildServerShareUrl({
      origin: 'http://localhost:8080',
      hostname: 'localhost',
      config: PUBLIC_CONFIG,
      lanHost: undefined,
      token: 'tok-1'
    })

    expect(result.status).toBe('no-lan-address')
  })
})
