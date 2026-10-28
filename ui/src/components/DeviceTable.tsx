import { useEffect, useState } from 'react'
import { BanIcon, CheckIcon, EyeIcon, EyeOffIcon, GaugeIcon, PencilIcon, RotateCcwIcon, WifiOffIcon } from 'lucide-react'
import type { Command, Device } from '../lib/protocol'
import type { Stats } from '../lib/useCore'
import { fmtByteRate } from '../lib/format'
import { Table, TableHeader, TableBody, TableRow, TableHead, TableCell } from './ui/table'
import { Badge } from './ui/badge'
import { Button } from './ui/button'
import { Input } from './ui/input'
import { Empty, EmptyHeader, EmptyMedia, EmptyTitle, EmptyDescription } from './ui/empty'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from './ui/tooltip'

interface Props {
	devices: Device[]
	stats: Record<string, Stats>
	selfIp?: string
	gatewayIp?: string
	send: (cmd: Command) => void
}

export function DeviceTable({ devices, stats, selfIp, gatewayIp, send }: Props) {
	const sorted = [...devices].sort((a, b) => ipNum(a.ip) - ipNum(b.ip))
	const hosts = sorted.filter((d) => d.ip !== selfIp && d.ip !== gatewayIp)
	const anyIntercepted = hosts.some((d) => d.intercepted)

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
		<Table className="table-fixed">
			<colgroup>
				<col className="w-10" />
				<col className="w-56" />
				<col className="w-44" />
				<col className="w-24" />
				<col className="w-24" />
				<col className="w-44" />
				<col />
			</colgroup>
			<TableHeader>
				<TableRow>
					<TableHead className="px-1">
						{hosts.length > 0 && (
							<TooltipProvider>
								<Tooltip>
									<TooltipTrigger>
										<Button size="icon-xs"
											variant="ghost"
											onClick={() => send({ cmd: anyIntercepted ? 'release_all' : 'intercept_all' })}
										>
											{anyIntercepted ? <EyeOffIcon /> : <EyeIcon />}
										</Button>
									</TooltipTrigger>
									<TooltipContent>
										{anyIntercepted ? 'Release all' : 'Intercept all'}
									</TooltipContent>
								</Tooltip>
							</TooltipProvider>
						)}
					</TableHead>
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
	const [editingNick, setEditingNick] = useState(false)
	const [nickValue, setNickValue] = useState(d.nickname ?? '')

	useEffect(() => {
		setDown(d.throttle?.down_kbps?.toString() ?? '')
		setUp(d.throttle?.up_kbps?.toString() ?? '')
	}, [d.throttle?.down_kbps, d.throttle?.up_kbps])

	useEffect(() => {
		setNickValue(d.nickname ?? '')
	}, [d.nickname])

	const downTrim = down.trim()
	const upTrim = up.trim()
	const origDown = d.throttle?.down_kbps?.toString() ?? ''
	const origUp = d.throttle?.up_kbps?.toString() ?? ''
	const throttleDirty = downTrim !== origDown || upTrim !== origUp

	const applyThrottle = () => {
		send({
			cmd: 'throttle',
			ip: d.ip,
			down_kbps: downTrim === '' ? null : Math.max(0, Number(downTrim)),
			up_kbps: upTrim === '' ? null : Math.max(0, Number(upTrim)),
		})
	}

	const isLimited = d.throttle?.down_kbps != null || d.throttle?.up_kbps != null

	const submitNick = () => {
		const trimmed = nickValue.trim()
		send({ cmd: 'set_nickname', mac: d.mac, nickname: trimmed === '' ? null : trimmed })
		setEditingNick(false)
	}

	const displayName = d.nickname || d.hostname || d.vendor || ''

	return (
		<TableRow
			className={`group ${d.cut ? 'bg-destructive/5 hover:bg-destructive/10' : d.intercepted ? 'bg-primary/5 hover:bg-primary/10' : ''}`}
		>
			<TableCell className="px-1 py-2">
				{controllable ? (
					<TooltipProvider>
						<Tooltip>
							<TooltipTrigger>
								<Button size="icon-xs"
									variant="ghost"
									onClick={() => send(d.intercepted ? { cmd: 'release', ip: d.ip } : { cmd: 'intercept', ip: d.ip })}
								>
									{d.intercepted ? <EyeOffIcon /> : <EyeIcon />}
								</Button>
							</TooltipTrigger>
							<TooltipContent>
								{d.intercepted ? 'Release' : 'Intercept'}
							</TooltipContent>
						</Tooltip>
					</TooltipProvider>
				) : null}
			</TableCell>
			<TableCell className="overflow-hidden py-3">
				<div className="min-w-0">
					<div className="mono truncate font-medium">{d.ip}</div>
				</div>
			</TableCell>
			<TableCell className="mono text-xs">
				{editingNick ? (
					<div className="flex items-center gap-1">
						<Input
							className="h-6 px-1 text-xs"
							value={nickValue}
							autoFocus
							onChange={(e) => setNickValue(e.target.value)}
							onKeyDown={(e) => {
								if (e.key === 'Enter') submitNick()
								if (e.key === 'Escape') {
									setNickValue(d.nickname ?? '')
									setEditingNick(false)
								}
							}}
						/>
						<TooltipProvider>
							<Tooltip>
								<TooltipTrigger>
									<Button size="icon-xs" variant="ghost" onClick={submitNick}>
										<CheckIcon />
									</Button>
								</TooltipTrigger>
								<TooltipContent>Save nickname</TooltipContent>
							</Tooltip>
						</TooltipProvider>
					</div>
				) : (
					<div className="flex items-center gap-1">
						<div className="min-w-0">
							<div className="truncate">{d.mac}</div>
							{displayName && <div className="truncate text-xs text-muted-foreground">{displayName}</div>}
						</div>
						{controllable && (
							<TooltipProvider>
								<Tooltip>
									<TooltipTrigger>
										<Button size="icon-xs"
											variant="ghost"
											className="opacity-0 group-hover:opacity-100"
											onClick={() => setEditingNick(true)}
										>
											<PencilIcon />
										</Button>
									</TooltipTrigger>
									<TooltipContent>Edit nickname</TooltipContent>
								</Tooltip>
							</TooltipProvider>
						)}
					</div>
				)}
			</TableCell>
			<TableCell className="mono text-right text-xs tabular-nums">{stats ? fmtByteRate(stats.down_bps) : '—'}</TableCell>
			<TableCell className="mono text-right text-xs tabular-nums">{stats ? fmtByteRate(stats.up_bps) : '—'}</TableCell>
			<TableCell>
				{role === 'self' && <Badge variant="secondary">this machine</Badge>}
				{role === 'gateway' && <Badge variant="default">gateway</Badge>}
				{role === 'host' && d.cut && <Badge variant="destructive">cut</Badge>}
				{role === 'host' && !d.cut && d.intercepted && <Badge variant="outline">intercepted</Badge>}
				{role === 'host' && !d.intercepted && <span className="text-xs text-muted-foreground">idle</span>}
			</TableCell>
			<TableCell>
				{controllable ? (
					<TooltipProvider>
						<div className="flex items-center gap-1">
							<Tooltip>
								<TooltipTrigger>
									<Button size="icon-xs"
										variant={d.cut ? 'destructive' : 'ghost'}
										disabled={!d.intercepted}
										onClick={() => send({ cmd: 'cut', ip: d.ip, enabled: !d.cut })}
									>
										<BanIcon />
									</Button>
								</TooltipTrigger>
								<TooltipContent>{d.cut ? 'Uncut' : 'Cut'}</TooltipContent>
							</Tooltip>
							<div className="flex items-center gap-1">
								<Input
									className="h-6 w-16 px-1 text-xs"
									type="number"
									min={0}
									placeholder="↓ kbps"
									value={down}
									disabled={!d.intercepted}
									onChange={(e) => setDown(e.target.value)}
								/>
								<Input
									className="h-6 w-16 px-1 text-xs"
									type="number"
									min={0}
									placeholder="↑ kbps"
									value={up}
									disabled={!d.intercepted}
									onChange={(e) => setUp(e.target.value)}
								/>
							</div>
							{isLimited && !throttleDirty && (
								<Tooltip>
									<TooltipTrigger>
										<Button size="icon-xs"
											variant="ghost"
											disabled={!d.intercepted}
											onClick={() => {
												setDown('')
												setUp('')
												send({
													cmd: 'throttle',
													ip: d.ip,
													down_kbps: null,
													up_kbps: null,
												})
											}}
										>
											<RotateCcwIcon />
										</Button>
									</TooltipTrigger>
									<TooltipContent>Unlimit</TooltipContent>
								</Tooltip>
							)}
							<Tooltip>
								<TooltipTrigger>
									<Button size="icon-xs"
										variant={throttleDirty ? 'default' : 'ghost'}
										disabled={!d.intercepted || !throttleDirty}
										onClick={applyThrottle}
									>
										<CheckIcon />
									</Button>
								</TooltipTrigger>
								<TooltipContent>{throttleDirty ? 'Apply throttle' : isLimited ? 'Limited' : 'Apply throttle'}</TooltipContent>
							</Tooltip>
						</div>
					</TooltipProvider>
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
