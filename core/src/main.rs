//! SelfishNet core daemon entrypoint.

mod engine;
mod net;
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

/// Privileged network core for SelfishNet.
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

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "selfishnet_core=info".into()),
        )
        .init();

    let args = Args::parse();

    let ctx = NetCtx::resolve(args.interface.as_deref())?;
    info!(
        "interface {} | self {} ({}) | gateway {}",
        ctx.iface.name, ctx.our_ip, ctx.our_mac, ctx.gateway_ip
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

    // Restore ARP caches + sysctls on Ctrl-C before exiting.
    let shutdown = app.engine.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            info!("shutting down…");
            shutdown.shutdown();
            std::process::exit(0);
        }
    });

    let bind = SocketAddr::new(args.bind, args.port);
    let serve = ws::serve(app.clone(), bind).await;

    // If the server loop ever returns, still clean up.
    app.engine.shutdown();
    serve
}
