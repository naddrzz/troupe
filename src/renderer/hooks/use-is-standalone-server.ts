import { useSyncExternalStore } from 'react'
import {
  getServerCapabilitySnapshot,
  isTauriContext,
  subscribeServerCapability
} from '@/lib/tauri-runtime'

/**
 * True iff the web client is connected to a standalone `troupe-server` (per
 * `GET /health` `standaloneServer: true`). Always `false` on Tauri desktop and
 * on desktop shared-live remote hosts.
 */
export function useIsStandaloneServer(): boolean {
  const { standaloneServer } = useSyncExternalStore(
    subscribeServerCapability,
    getServerCapabilitySnapshot()
  )
  return !isTauriContext() && standaloneServer
}
