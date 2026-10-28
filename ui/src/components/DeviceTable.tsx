import { useState } from 'react'
import type { Command, Device } from '../lib/protocol'
import type { Stats } from '../lib/useCore'
import { fmtByteRate } from '../lib/format'

interface Props {
  devices: Device[]
  stats: Record<string, Stats>
  selfIp?: string
  gatewayIp?: string
  send: (cmd: Command) => void
}

export function DeviceTable({ devices, stats, selfIp, gatewayIp, send }: Props) {
  const sorted = [...devices].sort((a, b) => ipNum(a.ip) - ipNum(b.ip))

  if (sorted.length === 0) {
    return <div className="empty">No devices yet. Hit <strong>Scan</strong> to sweep the subnet.</div>
  }

  return (
    <table className="devices">
      <thead>
        <tr>
          <th>Device</th>
          <th>MAC</th>
          <th className="num">Down</th>
          <th className="num">Up</th>
          <th>State</th>
          <th className="actions">Controls</th>
        </tr>
      </thead>
      <tbody>
        {sorted.map((d) => (
          <DeviceRow
            key={d.ip}
            device={d}
            stats={stats[d.ip]}
            role={d.ip === selfIp ? 'self' : d.ip === gatewayIp ? 'gateway' : 'host'}
            send={send}
          />
        ))}
      </tbody>
    </table>
  )
}

function DeviceRow({
  device: d,
  stats,
  role,
  send,
}: {
  device: Device
  stats?: Stats
  role: 'self' | 'gateway' | 'host'
  send: (cmd: Command) => void
}) {
  const [down, setDown] = useState<string>(d.throttle?.down_kbps?.toString() ?? '')
  const [up, setUp] = useState<string>(d.throttle?.up_kbps?.toString() ?? '')
  const controllable = role === 'host'

  const applyThrottle = () => {
    send({
      cmd: 'throttle',
      ip: d.ip,
      down_kbps: down.trim() === '' ? null : Math.max(0, Number(down)),
      up_kbps: up.trim() === '' ? null : Math.max(0, Number(up)),
    })
  }

  return (
    <tr className={d.cut ? 'cut' : d.intercepted ? 'intercepted' : ''}>
      <td>
        <div className="ip">{d.ip}</div>
        <div className="sub">{d.hostname ?? d.vendor ?? ''}</div>
      </td>
      <td className="mono">{d.mac}</td>
      <td className="num mono">{stats ? fmtByteRate(stats.down_bps) : '—'}</td>
      <td className="num mono">{stats ? fmtByteRate(stats.up_bps) : '—'}</td>
      <td>
        {role === 'self' && <span className="badge self">this machine</span>}
        {role === 'gateway' && <span className="badge gateway">gateway</span>}
        {role === 'host' && d.cut && <span className="badge danger">cut</span>}
        {role === 'host' && !d.cut && d.intercepted && <span className="badge warn">intercepted</span>}
        {role === 'host' && !d.intercepted && <span className="badge idle">idle</span>}
      </td>
      <td className="actions">
        {controllable ? (
          <div className="controls">
            <button
              className={d.intercepted ? 'on' : ''}
              onClick={() => send(d.intercepted ? { cmd: 'release', ip: d.ip } : { cmd: 'intercept', ip: d.ip })}
            >
              {d.intercepted ? 'Release' : 'Intercept'}
            </button>
            <button
              className={d.cut ? 'danger on' : 'danger'}
              disabled={!d.intercepted}
              onClick={() => send({ cmd: 'cut', ip: d.ip, enabled: !d.cut })}
            >
              {d.cut ? 'Uncut' : 'Cut'}
            </button>
            <span className="throttle">
              <input
                type="number"
                min={0}
                placeholder="↓ kbps"
                value={down}
                disabled={!d.intercepted}
                onChange={(e) => setDown(e.target.value)}
              />
              <input
                type="number"
                min={0}
                placeholder="↑ kbps"
                value={up}
                disabled={!d.intercepted}
                onChange={(e) => setUp(e.target.value)}
              />
              <button disabled={!d.intercepted} onClick={applyThrottle}>
                Limit
              </button>
            </span>
          </div>
        ) : (
          <span className="sub">—</span>
        )}
      </td>
    </tr>
  )
}

function ipNum(ip: string): number {
  return ip.split('.').reduce((acc, oct) => acc * 256 + Number(oct), 0)
}
