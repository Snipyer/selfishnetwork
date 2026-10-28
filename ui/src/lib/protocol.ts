// Mirror of core/src/protocol.rs. Keep the two in sync (see docs/protocol.md).

export interface Endpoint {
  ip: string
  mac: string
}

export interface Throttle {
  down_kbps: number | null
  up_kbps: number | null
}

export interface Device {
  ip: string
  mac: string
  hostname?: string
  vendor?: string
  intercepted: boolean
  cut: boolean
  throttle?: Throttle
}

export type Command =
  | { cmd: 'scan' }
  | { cmd: 'status' }
  | { cmd: 'intercept'; ip: string }
  | { cmd: 'release'; ip: string }
  | { cmd: 'cut'; ip: string; enabled: boolean }
  | { cmd: 'throttle'; ip: string; down_kbps: number | null; up_kbps: number | null }

export type Event =
  | { event: 'hello'; iface: string; gateway: Endpoint; self: Endpoint }
  | { event: 'scan_started' }
  | { event: 'device'; device: Device }
  | { event: 'scan_done'; count: number }
  | { event: 'device_update'; device: Device }
  | {
      event: 'stats'
      ip: string
      down_bps: number
      up_bps: number
      down_total: number
      up_total: number
    }
  | { event: 'error'; message: string }
