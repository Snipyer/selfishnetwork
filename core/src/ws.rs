//! WebSocket server: the control plane the UI talks to.
//!
//! Commands come in as JSON ([`Command`]); the core streams back [`Event`]s.
//! Device discovery is live; interception/cut/throttle currently update the
//! shared state and acknowledge (the data plane in `net::arp` / `net::forward`
//! is wired next), so the UI is fully exercisable end-to-end.

use crate::net::scan;
use crate::protocol::{Command, Event, Throttle};
use crate::state::AppState;
use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};

type Shared = Arc<Mutex<AppState>>;

/// Listen for UI connections forever.
pub async fn serve(state: Shared, bind: SocketAddr) -> Result<()> {
    let listener = TcpListener::bind(bind).await?;
    info!("listening on ws://{bind}");
    loop {
        let (stream, peer) = listener.accept().await?;
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(stream, state).await {
                warn!("connection {peer} ended: {e}");
            }
        });
    }
}

async fn handle(stream: TcpStream, state: Shared) -> Result<()> {
    let ws = tokio_tungstenite::accept_async(stream).await?;
    let (mut write, mut read) = ws.split();

    // Greet: who we are, plus whatever we already know.
    let (hello, snapshot) = {
        let s = state.lock().unwrap();
        (
            Event::Hello {
                iface: s.ctx.iface.name.clone(),
                gateway: s.gateway.clone(),
                own: s.own.clone(),
            },
            s.snapshot(),
        )
    };
    send(&mut write, &hello).await?;
    for device in snapshot {
        send(&mut write, &Event::Device { device }).await?;
    }

    while let Some(msg) = read.next().await {
        let msg = msg?;
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            Message::Ping(_) | Message::Pong(_) | Message::Binary(_) | Message::Frame(_) => continue,
        };
        let cmd: Command = match serde_json::from_str(&text) {
            Ok(c) => c,
            Err(e) => {
                send(&mut write, &Event::Error { message: format!("bad command: {e}") }).await?;
                continue;
            }
        };
        if let Err(e) = dispatch(cmd, &state, &mut write).await {
            send(&mut write, &Event::Error { message: e.to_string() }).await?;
        }
    }
    Ok(())
}

async fn dispatch<W>(cmd: Command, state: &Shared, write: &mut W) -> Result<()>
where
    W: SinkExt<Message> + Unpin,
    W::Error: std::error::Error + Send + Sync + 'static,
{
    match cmd {
        Command::Scan => run_scan(state, write).await?,
        Command::Status => {
            let snap = state.lock().unwrap().snapshot();
            for device in snap {
                send(write, &Event::Device { device }).await?;
            }
        }
        Command::Intercept { ip } => {
            emit_update(state, write, ip, |d| d.intercepted = true).await?;
            warn!("intercept {ip}: data plane not yet wired (scaffold)");
        }
        Command::Release { ip } => {
            emit_update(state, write, ip, |d| {
                d.intercepted = false;
                d.cut = false;
                d.throttle = Throttle::default();
            })
            .await?;
        }
        Command::Cut { ip, enabled } => {
            emit_update(state, write, ip, |d| d.cut = enabled).await?;
        }
        Command::Throttle { ip, down_kbps, up_kbps } => {
            emit_update(state, write, ip, |d| {
                d.throttle = Throttle { down_kbps, up_kbps };
            })
            .await?;
        }
    }
    Ok(())
}

async fn run_scan<W>(state: &Shared, write: &mut W) -> Result<()>
where
    W: SinkExt<Message> + Unpin,
    W::Error: std::error::Error + Send + Sync + 'static,
{
    send(write, &Event::ScanStarted).await?;

    let ctx = state.lock().unwrap().ctx.clone();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let handle = tokio::task::spawn_blocking(move || {
        scan::arp_sweep(&ctx, Duration::from_secs(3), |ip, mac| {
            let _ = tx.send((ip, mac));
        })
    });

    let mut count = 0;
    while let Some((ip, mac)) = rx.recv().await {
        let device = state.lock().unwrap().upsert(ip, mac);
        send(write, &Event::Device { device }).await?;
        count += 1;
    }
    handle.await??;
    send(write, &Event::ScanDone { count }).await?;
    Ok(())
}

async fn emit_update<W, F>(state: &Shared, write: &mut W, ip: std::net::Ipv4Addr, f: F) -> Result<()>
where
    W: SinkExt<Message> + Unpin,
    W::Error: std::error::Error + Send + Sync + 'static,
    F: FnOnce(&mut crate::protocol::Device),
{
    let updated = state.lock().unwrap().update(&ip, f);
    match updated {
        Some(device) => send(write, &Event::DeviceUpdate { device }).await?,
        None => send(write, &Event::Error { message: format!("unknown device {ip}") }).await?,
    }
    Ok(())
}

async fn send<W>(write: &mut W, event: &Event) -> Result<()>
where
    W: SinkExt<Message> + Unpin,
    W::Error: std::error::Error + Send + Sync + 'static,
{
    let json = serde_json::to_string(event)?;
    write.send(Message::Text(json)).await.map_err(anyhow::Error::from)?;
    Ok(())
}
