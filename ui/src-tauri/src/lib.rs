// SelfishNetwork desktop shell.
//
// The privileged work lives in a separate Rust daemon (`core/`); this Tauri
// shell only renders the React UI and manages that daemon's lifecycle so the
// user never has to start it by hand. The renderer talks to the daemon over a
// localhost WebSocket (ws://127.0.0.1:8734) and never touches raw sockets.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

use tauri::path::BaseDirectory;
use tauri::{Manager, RunEvent};

// cap_net_raw      — open AF_PACKET raw sockets (capture/inject).
// cap_net_admin    — network admin bits used around raw sockets.
// cap_dac_override — write the root-owned /proc/sys/net knobs (ip_forward,
//                    send_redirects) that userspace forwarding needs.
const CORE_CAPS: &str = "cap_net_raw,cap_net_admin,cap_dac_override+ep";

/// Holds the auto-started core daemon so it can be killed when the app exits.
#[derive(Default)]
struct CoreProcess(Mutex<Option<Child>>);

/// Resolves the core daemon binary, mirroring Electron's `app.isPackaged`
/// split. In a packaged build it's the bundled resource, which the deb
/// postinst (or the AppImage first-run path below) grants capabilities to. In
/// development it's the capability-granted release binary built in the
/// workspace (see scripts/setcap-core.sh) — not the un-setcap'd copy Tauri
/// stages next to the dev binary when it mirrors bundle resources.
fn core_binary_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    // CARGO_MANIFEST_DIR is ui/src-tauri at compile time.
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../core/target/release/selfishnet-core");
    if cfg!(debug_assertions) && dev.exists() {
        return Some(dev);
    }

    if let Ok(bundled) = app.path().resolve("bin/selfishnet-core", BaseDirectory::Resource) {
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

/// Launches the bundled core daemon. If another instance is already listening
/// on the port (e.g. started via scripts/dev.sh), the spawned process just
/// fails to bind and exits — the UI talks to whichever one got there first.
fn start_core(app: &tauri::AppHandle) {
    let Some(bin) = ensure_runnable_core_binary(app) else {
        eprintln!("[core] binary not found, skipping auto-start");
        return;
    };

    match Command::new(&bin).stdout(Stdio::inherit()).stderr(Stdio::inherit()).spawn() {
        Ok(child) => {
            *app.state::<CoreProcess>().0.lock().unwrap() = Some(child);
        }
        Err(e) => eprintln!("[core] failed to start: {e}"),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(CoreProcess::default())
        .setup(|app| {
            start_core(app.handle());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building SelfishNetwork")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                if let Some(mut child) = app.state::<CoreProcess>().0.lock().unwrap().take() {
                    let _ = child.kill();
                }
            }
        });
}
