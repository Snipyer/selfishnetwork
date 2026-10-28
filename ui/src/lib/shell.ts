import { getCurrentWindow } from '@tauri-apps/api/window'

// The desktop shell surface. Under Tauri the frontend drives the native window
// directly through the window API; the core daemon is auto-started by the Rust
// backend (see src-tauri/src/lib.rs). The renderer still talks to the core over
// a localhost WebSocket, so that contract is unchanged from the Electron build.

export const APP_VERSION = '0.1.0'
export const CORE_URL = 'ws://127.0.0.1:8734'

const appWindow = getCurrentWindow()

export const shell = {
  minimize: () => appWindow.minimize(),
  maximize: () => appWindow.toggleMaximize(),
  close: () => appWindow.close(),
}
