import { RadarIcon, TriangleAlertIcon, OctagonAlertIcon, SunIcon, MoonIcon, MinusIcon, SquareIcon, XIcon } from 'lucide-react'
import { useCore } from './lib/useCore'
import { useTheme } from './lib/useTheme'
import { shell, CORE_URL } from './lib/shell'
import { cn } from './lib/utils'
import { DeviceTable } from './components/DeviceTable'
import { Button } from './components/ui/button'
import { Alert, AlertDescription, AlertTitle } from './components/ui/alert'
import { TooltipProvider } from './components/ui/tooltip'
import appIcon from './assets/app-icon.png'

export default function App() {
	const { state, send } = useCore()
	const { theme, toggleTheme } = useTheme()
	const hostCount = state.devices.filter((d) => d.ip !== state.self?.ip && d.ip !== state.gateway?.ip).length

	return (
		<TooltipProvider>
			<div className="flex h-screen flex-col bg-background text-foreground">
				<header className="border-b border-border">
					<div className="flex h-9 items-center px-2" data-tauri-drag-region>
						<div className="flex flex-1 items-center justify-center gap-2" data-tauri-drag-region>
							<img src={appIcon} alt="SelfishNetwork" className="size-4" />
							<span className="text-xs font-medium tracking-tight">SelfishNetwork</span>
						</div>
						<div className="flex items-center gap-1">
							<Button
								variant="ghost"
								size="icon"
								aria-label="Minimize"
								onClick={() => shell.minimize()}
							>
								<MinusIcon />
							</Button>
							<Button
								variant="ghost"
								size="icon"
								aria-label="Maximize"
								onClick={() => shell.maximize()}
							>
								<SquareIcon />
							</Button>
							<Button
								variant="ghost"
								size="icon"
								aria-label="Close"
								onClick={() => shell.close()}
							>
								<XIcon />
							</Button>
						</div>
					</div>
					<div className="flex items-center justify-between gap-6 px-6 py-3.5">
						<div className="flex items-center gap-3">
							<Button disabled={!state.connected || state.scanning} onClick={() => send({ cmd: 'scan' })}>
								<RadarIcon data-icon="inline-start" />
								{state.scanning ? 'Scanning…' : 'Scan'}
							</Button>
							<span className={cn('size-1.5 rounded-full', state.connected ? 'bg-primary' : 'bg-destructive')} />
							<span className="text-xs text-muted-foreground">{state.connected ? 'Core connected' : 'Core offline'}</span>
						</div>
						<Button
							variant="outline"
							size="icon"
							aria-label={theme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'}
							onClick={toggleTheme}
						>
							{theme === 'dark' ? <SunIcon /> : <MoonIcon />}
						</Button>
					</div>

					{state.iface && (
						<div className="mono px-6 pb-2 text-xs text-muted-foreground">
							{state.iface} · self {state.self?.ip ?? '—'} · gateway {state.gateway?.ip ?? '—'} ·{' '}
							<span className="text-foreground">
								{hostCount} host{hostCount === 1 ? '' : 's'}
							</span>{' '}
							discovered
						</div>
					)}
				</header>

				<main className="flex-1 overflow-auto p-6">
					{!state.connected && (
						<Alert className="mb-4">
							<TriangleAlertIcon className="text-chart-4" />
							<AlertTitle>Core daemon unreachable</AlertTitle>
							<AlertDescription>
								Can&rsquo;t reach the core daemon at{' '}
								<code className="mono rounded-none bg-muted px-1 py-0.5">
									{CORE_URL}
								</code>
								. Start it with{' '}
								<code className="mono rounded-none bg-muted px-1 py-0.5">
									./scripts/setcap-core.sh &amp;&amp; selfishnet-core
								</code>
								. Retrying…
							</AlertDescription>
						</Alert>
					)}
					{state.lastError && (
						<Alert variant="destructive" className="mb-4">
							<OctagonAlertIcon />
							<AlertDescription>{state.lastError}</AlertDescription>
						</Alert>
					)}

					<DeviceTable
						devices={state.devices}
						stats={state.stats}
						selfIp={state.self?.ip}
						gatewayIp={state.gateway?.ip}
						send={send}
					/>
				</main>
			</div>
		</TooltipProvider>
	)
}
