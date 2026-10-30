import { RadarIcon, TriangleAlertIcon, OctagonAlertIcon, SunIcon, MoonIcon, MinusIcon, SquareIcon, XIcon, ArrowUpCircleIcon } from 'lucide-react'
import { useCore } from './lib/useCore'
import { useTheme } from './lib/useTheme'
import { useUpdateCheck } from './lib/useUpdateCheck'
import { shell, CORE_URL } from './lib/shell'
import { cn } from './lib/utils'
import { DeviceTable } from './components/DeviceTable'
import { Button } from './components/ui/button'
import { Alert, AlertDescription, AlertTitle } from './components/ui/alert'
import { TooltipProvider } from './components/ui/tooltip'
import { Toaster } from './components/ui/sonner'
import appIcon from './assets/app-icon.png'

export default function App() {
	const { state, send } = useCore()
	const { theme, toggleTheme } = useTheme()
	const { latestVersion, releaseUrl } = useUpdateCheck()
	const hostCount = state.devices.filter((d) => d.ip !== state.self?.ip && d.ip !== state.gateway?.ip).length

	return (
		<TooltipProvider>
			<div className="flex h-screen flex-col bg-background text-foreground">
				<header className="border-b border-border">
					<div className="relative flex h-9 items-center px-2" data-tauri-drag-region>
						<div className="pointer-events-none absolute inset-0 flex items-center justify-center gap-2" data-tauri-drag-region>
							<img src={appIcon} alt="SelfishNetwork" className="size-4" />
							<span className="text-xs font-medium tracking-tight">SelfishNetwork</span>
						</div>
						{/* spacer keeps the button + window controls pushed to the right */}
						<div className="flex-1" data-tauri-drag-region />
						{latestVersion && (
							<button
								type="button"
								onClick={() => shell.openExternal(releaseUrl)}
								title={`Version ${latestVersion} is available — open the release page`}
								className="mr-1 flex items-center gap-1 rounded-sm text-xs font-medium text-green-500 transition-colors hover:text-green-400"
							>
								<ArrowUpCircleIcon className="size-3.5" />
								New update available!
							</button>
						)}
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
								.{' '}
								{import.meta.env.DEV && (
									<>
										Start it with{' '}
										<code className="mono rounded-none bg-muted px-1 py-0.5">
											./scripts/setcap-core.sh &amp;&amp; selfishnet-core
										</code>
										.{' '}
									</>
						)}
								Retrying…
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
						history={state.history}
						selfIp={state.self?.ip}
						gatewayIp={state.gateway?.ip}
						send={send}
					/>
				</main>
				<Toaster theme={theme} position="bottom-right" />
			</div>
		</TooltipProvider>
	)
}
