import { ZapIcon, RadarIcon, TriangleAlertIcon, OctagonAlertIcon } from 'lucide-react'
import { useCore } from './lib/useCore'
import { DeviceTable } from './components/DeviceTable'
import { Button } from './components/ui/button'
import { Badge } from './components/ui/badge'
import { Alert, AlertDescription, AlertTitle } from './components/ui/alert'
import { TooltipProvider } from './components/ui/tooltip'

export default function App() {
  const { state, send } = useCore()
  const hostCount = state.devices.filter((d) => d.ip !== state.self?.ip && d.ip !== state.gateway?.ip).length

  return (
    <TooltipProvider>
      <div className="flex h-screen flex-col bg-background text-foreground">
        <header className="flex items-center gap-6 border-b border-border bg-card px-5 py-3">
          <div className="flex items-center gap-2 font-heading text-sm font-semibold">
            <ZapIcon className="size-4 text-primary" />
            <span>SelfishNet</span>
          </div>

          <div className="flex flex-1 gap-5">
            {state.iface && (
              <>
                <Info label="iface" value={state.iface} />
                <Info label="self" value={state.self?.ip ?? '—'} />
                <Info label="gateway" value={state.gateway?.ip ?? '—'} />
              </>
            )}
          </div>

          <div className="flex items-center gap-3">
            <Badge variant={state.connected ? 'default' : 'destructive'}>
              {state.connected ? 'core connected' : 'core offline'}
            </Badge>
            <Button disabled={!state.connected || state.scanning} onClick={() => send({ cmd: 'scan' })}>
              <RadarIcon data-icon="inline-start" />
              {state.scanning ? 'Scanning…' : 'Scan'}
            </Button>
          </div>
        </header>

        <main className="flex-1 overflow-auto p-5">
          {!state.connected && (
            <Alert className="mb-3.5">
              <TriangleAlertIcon className="text-chart-4" />
              <AlertTitle>Core daemon unreachable</AlertTitle>
              <AlertDescription>
                Can&rsquo;t reach the core daemon at{' '}
                <code className="mono rounded-none bg-muted px-1 py-0.5">
                  {window.selfishnet?.coreUrl ?? 'ws://127.0.0.1:8734'}
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
            <Alert variant="destructive" className="mb-3.5">
              <OctagonAlertIcon />
              <AlertDescription>{state.lastError}</AlertDescription>
            </Alert>
          )}

          <div className="mb-2.5 text-xs text-muted-foreground">
            {hostCount} host{hostCount === 1 ? '' : 's'} discovered
          </div>

          <DeviceTable
            devices={state.devices}
            stats={state.stats}
            selfIp={state.self?.ip}
            gatewayIp={state.gateway?.ip}
            send={send}
          />
        </main>

        <footer className="border-t border-border bg-card px-5 py-2 text-xs text-muted-foreground">
          Authorized use only — manage networks you own or administer.
        </footer>
      </div>
    </TooltipProvider>
  )
}

function Info({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex flex-col leading-tight">
      <span className="text-[11px] tracking-wide text-muted-foreground uppercase">{label}</span>
      <span className="mono text-xs">{value}</span>
    </div>
  )
}
