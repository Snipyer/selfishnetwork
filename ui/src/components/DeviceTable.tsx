import { useState } from 'react'
import { WifiOffIcon } from 'lucide-react'
import type { Command, Device } from '../lib/protocol'
import type { Stats } from '../lib/useCore'
import { fmtByteRate } from '../lib/format'
import { Table, TableHeader, TableBody, TableRow, TableHead, TableCell } from './ui/table'
import { Badge } from './ui/badge'
import { Button } from './ui/button'
import { Input } from './ui/input'
import { Empty, EmptyHeader, EmptyMedia, EmptyTitle, EmptyDescription } from './ui/empty'

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
    return (
      <Empty className="border border-dashed border-border">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <WifiOffIcon />
          </EmptyMedia>
          <EmptyTitle>No devices yet</EmptyTitle>
          <EmptyDescription>
            Hit <strong>Scan</strong> to sweep the subnet.
          </EmptyDescription>
        </EmptyHeader>
      </Empty>
    )
  }

  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>Device</TableHead>
          <TableHead>MAC</TableHead>
          <TableHead className="text-right">Down</TableHead>
          <TableHead className="text-right">Up</TableHead>
          <TableHead>State</TableHead>
          <TableHead>Controls</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {sorted.map((d) => (
          <DeviceRow
            key={d.ip}
            device={d}
            stats={stats[d.ip]}
            role={d.ip === selfIp ? 'self' : d.ip === gatewayIp ? 'gateway' : 'host'}
            send={send}
          />
        ))}
      </TableBody>
    </Table>
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
    <TableRow
      className={
        d.cut
          ? 'bg-destructive/5 hover:bg-destructive/10'
          : d.intercepted
            ? 'bg-primary/5 hover:bg-primary/10'
            : undefined
      }
    >
      <TableCell>
        <div className="font-medium">{d.ip}</div>
        <div className="text-xs text-muted-foreground">{d.hostname ?? d.vendor ?? ''}</div>
      </TableCell>
      <TableCell className="mono text-xs">{d.mac}</TableCell>
      <TableCell className="mono text-right text-xs">{stats ? fmtByteRate(stats.down_bps) : '—'}</TableCell>
      <TableCell className="mono text-right text-xs">{stats ? fmtByteRate(stats.up_bps) : '—'}</TableCell>
      <TableCell>
        {role === 'self' && <Badge variant="secondary">this machine</Badge>}
        {role === 'gateway' && <Badge variant="default">gateway</Badge>}
        {role === 'host' && d.cut && <Badge variant="destructive">cut</Badge>}
        {role === 'host' && !d.cut && d.intercepted && <Badge variant="outline">intercepted</Badge>}
        {role === 'host' && !d.intercepted && <Badge variant="ghost">idle</Badge>}
      </TableCell>
      <TableCell>
        {controllable ? (
          <div className="flex flex-wrap items-center gap-2">
            <Button
              size="sm"
              variant={d.intercepted ? 'default' : 'outline'}
              onClick={() => send(d.intercepted ? { cmd: 'release', ip: d.ip } : { cmd: 'intercept', ip: d.ip })}
            >
              {d.intercepted ? 'Release' : 'Intercept'}
            </Button>
            <Button
              size="sm"
              variant="destructive"
              disabled={!d.intercepted}
              onClick={() => send({ cmd: 'cut', ip: d.ip, enabled: !d.cut })}
            >
              {d.cut ? 'Uncut' : 'Cut'}
            </Button>
            <Input
              className="w-20"
              type="number"
              min={0}
              placeholder="↓ kbps"
              value={down}
              disabled={!d.intercepted}
              onChange={(e) => setDown(e.target.value)}
            />
            <Input
              className="w-20"
              type="number"
              min={0}
              placeholder="↑ kbps"
              value={up}
              disabled={!d.intercepted}
              onChange={(e) => setUp(e.target.value)}
            />
            <Button size="sm" variant="outline" disabled={!d.intercepted} onClick={applyThrottle}>
              Limit
            </Button>
          </div>
        ) : (
          <span className="text-xs text-muted-foreground">—</span>
        )}
      </TableCell>
    </TableRow>
  )
}

function ipNum(ip: string): number {
  return ip.split('.').reduce((acc, oct) => acc * 256 + Number(oct), 0)
}
