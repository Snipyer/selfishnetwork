// SelfishNetwork desktop shell.
//
// The privileged work lives in a separate Rust daemon (`core/`); this Tauri
// shell only renders the React UI and manages that daemon's lifecycle so the
// user never has to start it by hand. The renderer talks to the daemon over a
// localhost WebSocket (ws://127.0.0.1:8734) and never touches raw sockets.

use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tauri::path::BaseDirectory;
use tauri::{Manager, RunEvent};

// The control-plane port the core serves and the UI connects to; kept in sync
// with ui/src/lib/shell.ts (CORE_URL) and the core's default --port.
const CORE_PORT: u16 = 8734;

// cap_net_raw      — open AF_PACKET raw sockets (capture/inject).
// cap_net_admin    — network admin bits used around raw sockets.
// cap_dac_override — write the root-owned /proc/sys/net knobs (ip_forward,
//                    send_redirects) that userspace forwarding needs.
const CORE_CAPS: &str = "cap_net_raw,cap_net_admin,cap_dac_override+ep";

// The core daemon's file name differs by platform (Windows keeps the `.exe`).
#[cfg(windows)]
const CORE_BIN: &str = "selfishnet-core.exe";
#[cfg(not(windows))]
const CORE_BIN: &str = "selfishnet-core";

/// Holds the auto-started core daemon so it can be killed when the app exits.
/// `shutting_down` tells the supervisor thread (see `supervise_core`) to stop
/// respawning once the app is tearing down.
#[derive(Default)]
struct CoreProcess {
    child: Mutex<Option<Child>>,
    shutting_down: AtomicBool,
}

/// Resolves the core daemon binary, mirroring Electron's `app.isPackaged`
/// split. In a packaged build it's the bundled resource, which the deb
/// postinst (or the AppImage first-run path below) grants capabilities to. In
/// development it's the capability-granted release binary built in the
/// workspace (see scripts/setcap-core.sh) — not the un-setcap'd copy Tauri
/// stages next to the dev binary when it mirrors bundle resources.
fn core_binary_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    // CARGO_MANIFEST_DIR is ui/src-tauri at compile time.
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../core/target/release/{CORE_BIN}"));
    if cfg!(debug_assertions) && dev.exists() {
        return Some(dev);
    }

    if let Ok(bundled) = app
        .path()
        .resolve(format!("bin/{CORE_BIN}"), BaseDirectory::Resource)
    {
        if bundled.exists() {
            return Some(bundled);
        }
    }

    dev.exists().then_some(dev)
}

/// Returns true if the binary already carries the raw-socket capability.
fn has_capabilities(bin: &PathBuf) -> bool {
    Command::new("getcap")
        .arg(bin)
        .output()
        .map(|out| out.status.success() && String::from_utf8_lossy(&out.stdout).contains("cap_net_raw"))
        .unwrap_or(false)
}

/// On a deb install the postinst script already set capabilities on the bundled
/// binary (see ui/build/deb-after-install.sh) — nothing more to do. An
/// AppImage, though, mounts its payload as a read-only squashfs, so the bundled
/// binary can never be setcap'd in place. For that case, copy it to a writable
/// per-user location once and grant it capabilities there via a one-time
/// graphical pkexec/polkit prompt, then reuse that copy on every later launch
/// (re-copying only when the bundled binary changes, e.g. after an app update).
fn ensure_runnable_core_binary(app: &tauri::AppHandle) -> Option<PathBuf> {
    let bundled = core_binary_path(app)?;
    if std::env::var_os("APPIMAGE").is_none() {
        return Some(bundled);
    }

    let dest_dir = app.path().app_local_data_dir().ok()?.join("bin");
    let dest = dest_dir.join("selfishnet-core");

    let bundled_bytes = std::fs::read(&bundled).ok()?;
    let up_to_date = std::fs::read(&dest).map(|cur| cur == bundled_bytes).unwrap_or(false);

    if !up_to_date {
        if let Err(e) = std::fs::create_dir_all(&dest_dir).and_then(|_| std::fs::write(&dest, &bundled_bytes)) {
            eprintln!("[core] failed to stage binary: {e}");
            return None;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
        }
    }

    if !up_to_date || !has_capabilities(&dest) {
        println!("[core] requesting raw-socket capabilities via pkexec (one-time authentication prompt)…");
        let status = Command::new("pkexec").args(["setcap", CORE_CAPS]).arg(&dest).status();
        match status {
            Ok(s) if s.success() => {}
            _ => eprintln!("[core] pkexec setcap failed or was declined — scanning/forwarding will not work until granted"),
        }
    }

    Some(dest)
}

/// True if something is already accepting connections on the core's port — a
/// core started via scripts/dev.sh, an orphaned daemon from a previous run, or
/// the one we spawned ourselves. We adopt it rather than fighting over the port.
fn core_port_served() -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], CORE_PORT));
    TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
}

/// Spawns one core daemon process with the shell's stdio, so packaged runs log
/// through to the parent (and `core.log`).
fn spawn_core(bin: &PathBuf) -> std::io::Result<Child> {
    let mut cmd = Command::new(bin);
    cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    // The core is a console binary; without this it pops a visible console
    // window when launched from the (windowed) shell on Windows.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    // Tie the core's lifetime to ours: if the shell dies in any way the kernel
    // couldn't give us a chance to clean up after (a crash, SIGKILL, a yanked
    // `tauri dev`), deliver a signal to the core so it restores network state
    // and exits — instead of lingering as an orphan that squats
    // ws://127.0.0.1:8734 and makes every future launch report the core
    // unreachable.
    //
    // We use SIGINT, not SIGTERM: a core spawned from this (GTK/glib) shell does
    // not install a SIGTERM handler (the inherited signal state swallows it), so
    // SIGTERM would hard-kill it before it could restore ARP/forwarding — but
    // SIGINT is caught, so the core's shutdown path runs. `stop_core` relies on
    // the same signal for the orderly-close case.
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGINT as libc::c_ulong, 0, 0, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                // Close the fork/exec race: if the shell already died before we
                // armed the signal, we've been reparented to init — stop now.
                if libc::getppid() == 1 {
                    libc::raise(libc::SIGINT);
                }
                Ok(())
            });
        }
    }
    cmd.spawn()
}

/// Starts the core daemon and keeps it running for the app's lifetime.
///
/// A one-shot spawn is fragile: if any other process is already bound to the
/// port when we launch (commonly an orphaned daemon left behind by a previous
/// `tauri dev`/app run), our core exits with "address already in use" and the
/// UI is stuck on "core unreachable" forever — the app only works while some
/// *other* core happens to be alive. Instead we supervise:
///
///   * if the port is already served, adopt that core and don't spawn (no
///     spinning on a port conflict with a dev-stack core);
///   * otherwise spawn our own and wait for it;
///   * when it (or an adopted core) goes away, reclaim the port by spawning
///     again. Paired with the UI's auto-reconnect, the app self-heals.
fn start_core(app: &tauri::AppHandle) {
    let Some(bin) = ensure_runnable_core_binary(app) else {
        eprintln!("[core] binary not found, skipping auto-start");
        return;
    };
    let app = app.clone();
    std::thread::spawn(move || supervise_core(app, bin));
}

/// Stops the core we spawned, giving it a chance to undo what it applied.
///
/// A plain `kill()` is SIGKILL, which the core can't trap — it would die with
/// the targets' ARP caches still poisoned and the host's IP-forwarding knobs
/// still flipped. So on Unix we send SIGINT (the core traps it and restores
/// network state), wait briefly for it to exit, and only force-kill if it won't
/// go. SIGINT rather than SIGTERM because a core spawned from this GTK shell
/// doesn't catch SIGTERM (see `spawn_core`). On other platforms we fall back to
/// the hard kill.
fn stop_core(child: &mut Child) {
    #[cfg(unix)]
    {
        let pid = child.id() as libc::pid_t;
        unsafe {
            libc::kill(pid, libc::SIGINT);
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
            match child.try_wait() {
                Ok(Some(_)) => return, // exited cleanly after restoring state
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(_) => break,
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn supervise_core(app: tauri::AppHandle, bin: PathBuf) {
    let state = app.state::<CoreProcess>();
    loop {
        if state.shutting_down.load(Ordering::SeqCst) {
            return;
        }

        // Someone's already serving (dev stack, or a core we can't see as our
        // child). Leave it be and re-check periodically.
        if core_port_served() {
            std::thread::sleep(Duration::from_secs(2));
            continue;
        }

        match spawn_core(&bin) {
            Ok(child) => {
                *state.child.lock().unwrap() = Some(child);
                // Poll for exit rather than a blocking wait(), so the Exit
                // handler can take and kill the child out from under us.
                loop {
                    if state.shutting_down.load(Ordering::SeqCst) {
                        return;
                    }
                    let mut guard = state.child.lock().unwrap();
                    match guard.as_mut() {
                        Some(child) => match child.try_wait() {
                            Ok(None) => {} // still running
                            _ => {
                                // Exited (or we can't tell): clear it and respawn.
                                *guard = None;
                                break;
                            }
                        },
                        None => return, // taken by the Exit handler
                    }
                    drop(guard);
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
            Err(e) => eprintln!("[core] failed to start: {e}"),
        }

        // Back off before the next attempt so a core that dies immediately
        // (e.g. no usable interface) doesn't spin.
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// Remove the Windows 11 non-client border. On an undecorated (`decorations:
/// false`) window Windows still paints a 1px DWM frame in a light system color,
/// which shows as a thin white hairline on the sides/bottom of our dark window.
/// `DWMWA_BORDER_COLOR = DWMWA_COLOR_NONE` drops it. No-op on Windows 10 (the
/// attribute is unsupported there and the call just returns an error we ignore).
#[cfg(windows)]
fn strip_window_border(app: &tauri::AppHandle) {
    use windows_sys::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
    };
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let hwnd = hwnd.0 as *mut core::ffi::c_void;
    unsafe {
        // Paint the frame border our background colour (BGR 0x00RRGGBB → 0x161110
        // for #0e1116) instead of the light default, so no light hairline shows.
        let color: u32 = 0x0016_110e;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR as u32,
            &color as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        );
        // Square the corners: Win11's rounded corners leave an anti-aliased light
        // edge against our dark window.
        let corner: i32 = DWMWCP_DONOTROUND;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &corner as *const i32 as *const core::ffi::c_void,
            std::mem::size_of::<i32>() as u32,
        );
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(CoreProcess::default())
        .setup(|app| {
            #[cfg(windows)]
            strip_window_border(app.handle());
            start_core(app.handle());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building SelfishNetwork")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                let state = app.state::<CoreProcess>();
                // Stop the supervisor before taking the child, so it doesn't
                // spawn a replacement as we shut down.
                state.shutting_down.store(true, Ordering::SeqCst);
                let child = state.child.lock().unwrap().take();
                if let Some(mut child) = child {
                    stop_core(&mut child);
                }
            }
        });
}
