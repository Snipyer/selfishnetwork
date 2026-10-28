import { useCallback, useEffect, useRef, useState } from 'react'
import type { Command, Device, Endpoint, Event } from './protocol'
import { CORE_URL } from './shell'

const RECONNECT_MS = 1500

export interface Stats {
  down_bps: number
  up_bps: number
  down_total: number
  up_total: number
}

export interface CoreState {
  connected: boolean
  scanning: boolean
  iface?: string
  gateway?: Endpoint
  self?: Endpoint
  devices: Device[]
  stats: Record<string, Stats>
  lastError?: string
}

/**
 * Manages the WebSocket connection to the core: auto-reconnect, event folding
 * into state, and a typed `send` for commands.
 */
export function useCore() {
  const [state, setState] = useState<CoreState>({
    connected: false,
    scanning: false,
    devices: [],
    stats: {},
  })
  const wsRef = useRef<WebSocket | null>(null)
  const devicesRef = useRef<Map<string, Device>>(new Map())

  const syncDevices = useCallback(() => {
    setState((s) => ({ ...s, devices: [...devicesRef.current.values()] }))
  }, [])

  const apply = useCallback(
    (ev: Event) => {
      switch (ev.event) {
        case 'hello':
          setState((s) => ({ ...s, iface: ev.iface, gateway: ev.gateway, self: ev.self }))
          break
        case 'scan_started':
          setState((s) => ({ ...s, scanning: true }))
          break
        case 'scan_done':
          setState((s) => ({ ...s, scanning: false }))
          break
        case 'device':
        case 'device_update':
          devicesRef.current.set(ev.device.ip, ev.device)
          syncDevices()
          break
        case 'stats':
          setState((s) => ({
            ...s,
            stats: {
              ...s.stats,
              [ev.ip]: {
                down_bps: ev.down_bps,
                up_bps: ev.up_bps,
                down_total: ev.down_total,
                up_total: ev.up_total,
              },
            },
          }))
          break
        case 'error':
          setState((s) => ({ ...s, lastError: ev.message }))
          break
      }
    },
    [syncDevices],
  )

  useEffect(() => {
    let closed = false
    let timer: ReturnType<typeof setTimeout>

    const connect = () => {
      const ws = new WebSocket(CORE_URL)
      wsRef.current = ws

      ws.onopen = () => setState((s) => ({ ...s, connected: true, lastError: undefined }))
      ws.onmessage = (msg) => {
        try {
          apply(JSON.parse(msg.data as string) as Event)
        } catch {
          /* ignore malformed frames */
        }
      }
      ws.onclose = () => {
        setState((s) => ({ ...s, connected: false, scanning: false }))
        if (!closed) timer = setTimeout(connect, RECONNECT_MS)
      }
      ws.onerror = () => ws.close()
    }

    connect()
    return () => {
      closed = true
      clearTimeout(timer)
      wsRef.current?.close()
    }
  }, [apply])

  const send = useCallback((cmd: Command) => {
    const ws = wsRef.current
    if (ws && ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(cmd))
  }, [])

  return { state, send }
}
