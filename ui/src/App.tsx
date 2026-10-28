import { useCore } from './lib/useCore'
import { DeviceTable } from './components/DeviceTable'

export default function App() {
  const { state, send } = useCore()
  const hostCount = state.devices.filter((d) => d.ip !== state.self?.ip && d.ip !== state.gateway?.ip).length

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <span className="logo">⚡</span>
          <span className="name">SelfishNet</span>
        </div>
        <div className="netinfo">
          {state.iface && (
            <>
              <Info label="iface" value={state.iface} />
              <Info label="self" value={state.self?.ip ?? '—'} />
              <Info label="gateway" value={state.gateway?.ip ?? '—'} />
            </>
          )}
        </div>
        <div className="right">
          <span className={`conn ${state.connected ? 'ok' : 'down'}`}>
            {state.connected ? 'core connected' : 'core offline'}
          </span>
          <button
            className="scan"
            disabled={!state.connected || state.scanning}
            onClick={() => send({ cmd: 'scan' })}
          >
            {state.scanning ? 'Scanning…' : 'Scan'}
          </button>
        </div>
      </header>

      <main>
        {!state.connected && (
          <div className="banner warn">
            Can't reach the core daemon at <code>{window.selfishnet?.coreUrl ?? 'ws://127.0.0.1:8734'}</code>.
            Start it with <code>./scripts/setcap-core.sh &amp;&amp; selfishnet-core</code>. Retrying…
          </div>
        )}
        {state.lastError && <div className="banner error">{state.lastError}</div>}

        <div className="count">{hostCount} host{hostCount === 1 ? '' : 's'} discovered</div>

        <DeviceTable
          devices={state.devices}
          stats={state.stats}
          selfIp={state.self?.ip}
          gatewayIp={state.gateway?.ip}
          send={send}
        />
      </main>

      <footer>
        <span>Authorized use only — manage networks you own or administer.</span>
      </footer>
    </div>
  )
}

function Info({ label, value }: { label: string; value: string }) {
  return (
    <span className="info">
      <span className="k">{label}</span>
      <span className="v mono">{value}</span>
    </span>
  )
}
