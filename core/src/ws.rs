//! WebSocket server: the control plane the UI talks to.
//!
//! Each connection subscribes to the app-wide event broadcast, so engine
//! `stats` and any client's `device_update` reach every client. Commands mutate
//! the device table + drive the [`Engine`](crate::engine::Engine) and publish
//! their results back through the broadcast.

use crate::net::scan;
use crate::protocol::{Command, Event, Throttle};
use crate::state::App;
use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast::error::RecvError;
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};

/// Listen for UI connections forever.
pub async fn serve(app: Arc<App>, bind: SocketAddr) -> Result<()> {
    let listener = TcpListener::bind(bind).await?;
    info!("listening on ws://{bind}");
    loop {
        let (stream, peer) = listener.accept().await?;
        let app = app.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(stream, app).await {
                warn!("connection {peer} ended: {e}");
            }
        });
    }
}

async fn handle(stream: TcpStream, app: Arc<App>) -> Result<()> {
    let ws = tokio_tungstenite::accept_async(stream).await?;
    let (mut write, mut read) = ws.split();
    let mut events = app.events.subscribe();

    // Greet with our identity and whatever we already know.
    send(
        &mut write,
        &Event::Hello {
            iface: app.ctx.iface_label(),
            gateway: app.gateway.clone(),
            own: app.own.clone(),
        },
    )
    .await?;
    for device in app.snapshot() {
        send(&mut write, &Event::Device { device }).await?;
    }

    loop {
        tokio::select! {
            biased;
            incoming = read.next() => match incoming {
                Some(Ok(Message::Text(txt))) => {
                    match serde_json::from_str::<Command>(&txt) {
                        Ok(cmd) => {
                            if let Err(e) = dispatch(&app, cmd, &mut write).await {
                                send(&mut write, &Event::Error { message: e.to_string() }).await?;
                            }
                        }
                        Err(e) => {
                            send(&mut write, &Event::Error { message: format!("bad command: {e}") }).await?;
                        }
                    }
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {}
                Some(Err(e)) => { warn!("read error: {e}"); break; }
            },
            ev = events.recv() => match ev {
                Ok(ev) => send(&mut write, &ev).await?,
                Err(RecvError::Lagged(n)) => warn!("client lagged, dropped {n} events"),
                Err(RecvError::Closed) => break,
            },
        }
    }
    Ok(())
}

async fn dispatch<W>(app: &Arc<App>, cmd: Command, write: &mut W) -> Result<()>
where
    W: SinkExt<Message> + Unpin,
    W::Error: std::error::Error + Send + Sync + 'static,
{
    match cmd {
        Command::Scan => run_scan(app),
        Command::Status => {
            for device in app.snapshot() {
                send(write, &Event::Device { device }).await?;
            }
        }
        Command::Intercept { ip } => {
            let mac = app
                .device_mac(&ip)
                .ok_or_else(|| anyhow::anyhow!("unknown device {ip} — scan first"))?;
            app.engine.intercept(ip, mac)?;
            publish_update(app, ip, |d| d.intercepted = true);
        }
        Command::Release { ip } => {
            app.engine.release(ip);
            publish_update(app, ip, |d| {
                d.intercepted = false;
                d.cut = false;
                d.throttle = Throttle::default();
            });
        }
        Command::InterceptAll => {
            let devices = app.snapshot();
            for d in devices {
                if let Ok(mac) = d.mac.parse() {
                    if let Err(e) = app.engine.intercept(d.ip, mac) {
                        let _ = e;
                    }
                    publish_update(app, d.ip, |dev| dev.intercepted = true);
                }
            }
        }
        Command::ReleaseAll => {
            for d in app.snapshot() {
                app.engine.release(d.ip);
                publish_update(app, d.ip, |dev| {
                    dev.intercepted = false;
                    dev.cut = false;
                    dev.throttle = Throttle::default();
                });
            }
        }
        Command::Cut { ip, enabled } => {
            app.engine.set_cut(ip, enabled);
            publish_update(app, ip, |d| d.cut = enabled);
        }
        Command::Throttle { ip, down_kbps, up_kbps } => {
            app.engine.set_throttle(ip, down_kbps, up_kbps);
            publish_update(app, ip, |d| d.throttle = Throttle { down_kbps, up_kbps });
        }
        Command::SetNickname { mac, nickname } => {
            if let Some(updated) = app.set_nickname(&mac, nickname.clone()) {
                // Also publish update for the IP if we can find it
                app.broadcast(Event::DeviceUpdate { device: updated.clone() });
                // Try to find by IP in snapshot
                for d in app.snapshot() {
                    if d.mac.eq_ignore_ascii_case(&mac) {
                        app.broadcast(Event::DeviceUpdate { device: d });
                        break;
                    }
                }
            } else {
                // Update any matching device in snapshot
                let nick_trim = nickname.as_ref().map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
                for d in app.snapshot() {
                    if d.mac.eq_ignore_ascii_case(&mac) {
                        let mut dev = d.clone();
                        dev.nickname = nick_trim.clone();
                        app.broadcast(Event::DeviceUpdate { device: dev });
                        break;
                    }
                }
            }
        }
    }
    Ok(())
}

/// Kick off an ARP sweep in the background, publishing results to all clients.
fn run_scan(app: &Arc<App>) {
    let app = app.clone();
    app.broadcast(Event::ScanStarted);
    tokio::spawn(async move {
        let ctx = app.ctx.clone();
        let sweep_app = app.clone();
        let result = tokio::task::spawn_blocking(move || {
            scan::arp_sweep(&ctx, Duration::from_secs(6), |ip, mac| {
                sweep_app.engine.note_mac(ip, mac);
                let device = sweep_app.upsert(ip, mac);
                sweep_app.broadcast(Event::Device { device });
            })
        })
        .await;

        let count = app.snapshot().len();
        match result {
            Ok(Ok(())) => app.broadcast(Event::ScanDone { count }),
            Ok(Err(e)) => app.broadcast(Event::Error { message: format!("scan failed: {e}") }),
            Err(e) => app.broadcast(Event::Error { message: format!("scan task failed: {e}") }),
        }
    });
}

/// Apply a UI-model change and broadcast it (or an error if unknown).
fn publish_update<F: FnOnce(&mut crate::protocol::Device)>(app: &Arc<App>, ip: std::net::Ipv4Addr, f: F) {
    match app.update_device(&ip, f) {
        Some(device) => app.broadcast(Event::DeviceUpdate { device }),
        None => app.broadcast(Event::Error { message: format!("unknown device {ip}") }),
    }
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
