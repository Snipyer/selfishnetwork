import { useCallback, useEffect, useRef, useState } from 'react'
import { toast } from 'sonner'
import type { Command, Device, Endpoint, Event } from './protocol'
import { CORE_URL } from './shell'

const RECONNECT_MS = 1500
/** Samples kept per device for the bandwidth sparkline (~60s at 1 sample/s). */
const HISTORY_LEN = 60

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
  /** Rolling total-throughput (down+up bps) ring buffer per device IP. */
  history: Record<string, number[]>
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
    history: {},
  })
  const wsRef = useRef<WebSocket | null>(null)
  const devicesRef = useRef<Map<string, Device>>(new Map())
  // New-device alerts: MACs seen so far, and whether the initial scan has
  // settled so we don't toast for the baseline that streams in on connect.
  const seenMacs = useRef<Set<string>>(new Set())
  const baselineReady = useRef(false)

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
          // The first completed scan establishes the baseline; after this,
          // any unseen MAC is a device that joined mid-session.
          baselineReady.current = true
          setState((s) => ({ ...s, scanning: false }))
          break
        case 'device':
        case 'device_update':
          if (ev.device.mac && !seenMacs.current.has(ev.device.mac)) {
            seenMacs.current.add(ev.device.mac)
            if (baselineReady.current) {
              const name = ev.device.nickname || ev.device.hostname || ev.device.vendor
              toast('New device joined', {
                description: `${name ? `${name} · ` : ''}${ev.device.ip} · ${ev.device.mac}`,
              })
            }
          }
          devicesRef.current.set(ev.device.ip, ev.device)
          syncDevices()
          break
        case 'stats':
          setState((s) => {
            const prev = s.history[ev.ip] ?? []
            const next = [...prev, ev.down_bps + ev.up_bps].slice(-HISTORY_LEN)
            return {
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
              history: { ...s.history, [ev.ip]: next },
            }
          })
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
