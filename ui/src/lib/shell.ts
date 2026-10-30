import { getCurrentWindow } from '@tauri-apps/api/window'
import { getVersion } from '@tauri-apps/api/app'
import { openUrl } from '@tauri-apps/plugin-opener'

// The desktop shell surface. Under Tauri the frontend drives the native window
// directly through the window API; the core daemon is auto-started by the Rust
// backend (see src-tauri/src/lib.rs). The renderer still talks to the core over
// a localhost WebSocket, so that contract is unchanged from the Electron build.

export const CORE_URL = 'ws://127.0.0.1:8734'

const appWindow = getCurrentWindow()

export const shell = {
  minimize: () => appWindow.minimize(),
  maximize: () => appWindow.toggleMaximize(),
  close: () => appWindow.close(),
  // The app's own version. The single source of truth is tauri.conf.json,
  // read at runtime from the bundle; this rejects when the Tauri API is
  // unavailable (e.g. a plain-browser dev server) rather than inventing one.
  version: () => getVersion(),
  // Open a URL in the user's default browser, not inside the app webview.
  openExternal: (url: string) => openUrl(url),
}
