//! SelfishNetwork core daemon entrypoint.

mod config;
mod engine;
mod net;
mod platform;
mod protocol;
mod state;
mod ws;

use anyhow::Result;
use clap::Parser;
use net::NetCtx;
use state::App;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

/// Privileged network core for SelfishNetwork.
#[derive(Parser, Debug)]
#[command(name = "selfishnet-core", version)]
struct Args {
    /// Network interface to use (default: the one with the default route).
    #[arg(short, long)]
    interface: Option<String>,

    /// Address to bind the WebSocket server to.
    #[arg(short, long, default_value = "127.0.0.1")]
    bind: IpAddr,

    /// Port for the WebSocket server.
    #[arg(short, long, default_value_t = 8734)]
    port: u16,
}

/// Logs to stderr and, so packaged (console-less) runs can be diagnosed, also
/// to `<local-data>/SelfishNetwork/core.log` (e.g. `%LOCALAPPDATA%\SelfishNetwork`
/// on Windows, `~/.local/share/SelfishNetwork` on Linux).
fn init_tracing() {
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "selfishnet_core=info".into());

    let file_layer = dirs::data_local_dir().and_then(|base| {
        let dir = base.join("SelfishNetwork");
        std::fs::create_dir_all(&dir).ok()?;
        let appender = tracing_appender::rolling::never(&dir, "core.log");
        Some(fmt::layer().with_ansi(false).with_writer(appender))
    });

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(file_layer)
        .init();
}

/// Resolves when the process is asked to stop. On Unix that's SIGTERM or SIGINT
/// (the shell uses SIGTERM; Ctrl-C from a terminal is SIGINT); elsewhere it's
/// the platform's Ctrl-C / close event.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        // If a handler can't be installed we still want Ctrl-C to work, so fall
        // back to a future that never resolves for the missing one.
        let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
        let mut int = signal(SignalKind::interrupt()).expect("install SIGINT handler");
        tokio::select! {
            _ = term.recv() => {}
            _ = int.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let args = Args::parse();

    let ctx = NetCtx::resolve(args.interface.as_deref())?;
    info!(
        "interface {} ({}) | self {} ({}) | netmask {} | gateway {} | {} candidate hosts",
        ctx.iface.name,
        ctx.iface_label(),
        ctx.our_ip,
        ctx.our_mac,
        ctx.netmask,
        ctx.gateway_ip,
        ctx.subnet_hosts().len(),
    );

    // Resolve the gateway's MAC once; non-fatal if it times out.
    let gateway_mac = match net::scan::resolve_mac(&ctx, ctx.gateway_ip, Duration::from_secs(2)) {
        Ok(Some(mac)) => {
            info!("gateway MAC {mac}");
            Some(mac)
        }
        Ok(None) => {
            warn!("gateway MAC not resolved (ARP timeout) — scan to learn it");
            None
        }
        Err(e) => {
            warn!("gateway MAC resolution failed: {e}");
            None
        }
    };

    let app = Arc::new(App::new(ctx, gateway_mac));
    app.engine.start();

    // Restore ARP caches + forwarding controls before exiting on any orderly
    // stop signal. The desktop shell stops us with SIGINT on app close, and the
    // kernel sends us SIGINT via PR_SET_PDEATHSIG if the shell dies unexpectedly;
    // SIGTERM is handled too for other launchers (e.g. systemd). Without this
    // either case would leave the targets' ARP caches poisoned and the host's
    // IP-forwarding knobs flipped.
    let shutdown = app.engine.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        info!("shutting down…");
        shutdown.shutdown();
        std::process::exit(0);
    });

    let bind = SocketAddr::new(args.bind, args.port);
    let serve = ws::serve(app.clone(), bind).await;

    // If the server loop ever returns, still clean up.
    app.engine.shutdown();
    serve
}
