import { app, BrowserWindow, ipcMain } from 'electron'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawn, spawnSync, type ChildProcess } from 'node:child_process'
import fs from 'node:fs'
import crypto from 'node:crypto'

const __dirname = path.dirname(fileURLToPath(import.meta.url))

// vite-plugin-electron sets this in dev; in a packaged build we load the file.
const DEV_URL = process.env.VITE_DEV_SERVER_URL

const CORE_CAPS = 'cap_net_raw,cap_net_admin,cap_dac_override+ep'

let coreProcess: ChildProcess | null = null

function coreBinaryPath(): string | null {
  const candidate = app.isPackaged
    ? path.join(process.resourcesPath, 'bin', 'selfishnet-core')
    : path.join(__dirname, '../../core/target/release/selfishnet-core')
  return fs.existsSync(candidate) ? candidate : null
}

function hasCapabilities(bin: string): boolean {
  const res = spawnSync('getcap', [bin], { encoding: 'utf8' })
  return res.status === 0 && res.stdout.includes('cap_net_raw')
}

// On a deb install the postinst script already set capabilities directly on
// the bundled binary (see build/deb-after-install.sh) — nothing more to do.
// An AppImage, though, mounts its payload as a read-only squashfs, so the
// bundled binary can never be setcap'd in place. For that case, copy it to a
// writable per-user location once and grant it capabilities there via a
// one-time graphical pkexec/polkit prompt, then reuse that copy on every
// later launch (re-copying only when the bundled binary changes, e.g. after
// an app update).
function ensureRunnableCoreBinary(): string | null {
  const bundled = coreBinaryPath()
  if (!bundled || !process.env.APPIMAGE) return bundled

  const destDir = path.join(app.getPath('userData'), 'bin')
  const dest = path.join(destDir, 'selfishnet-core')
  const hashFile = `${dest}.sha256`
  const bundledHash = crypto.createHash('sha256').update(fs.readFileSync(bundled)).digest('hex')
  const upToDate = fs.existsSync(dest) && fs.existsSync(hashFile) && fs.readFileSync(hashFile, 'utf8').trim() === bundledHash

  if (!upToDate) {
    fs.mkdirSync(destDir, { recursive: true })
    fs.copyFileSync(bundled, dest)
    fs.chmodSync(dest, 0o755)
    fs.writeFileSync(hashFile, bundledHash)
  }

  if (!upToDate || !hasCapabilities(dest)) {
    console.log('[core] requesting raw-socket capabilities via pkexec (one-time authentication prompt)…')
    const res = spawnSync('pkexec', ['setcap', CORE_CAPS, dest], { stdio: 'inherit' })
    if (res.status !== 0) {
      console.error('[core] pkexec setcap failed or was declined — scanning/forwarding will not work until granted')
    }
  }

  return dest
}

// Launches the bundled core daemon so the user never has to run it by hand.
// If another instance is already listening on the port (e.g. started via
// scripts/dev.sh), the spawned process just fails to bind and exits — the UI
// talks to whichever one got there first.
function startCore() {
  const bin = ensureRunnableCoreBinary()
  if (!bin) {
    console.warn('[core] binary not found, skipping auto-start:', bin)
    return
  }

  coreProcess = spawn(bin, [], { stdio: ['ignore', 'pipe', 'pipe'] })
  coreProcess.stdout?.on('data', (d) => process.stdout.write(`[core] ${d}`))
  coreProcess.stderr?.on('data', (d) => process.stderr.write(`[core] ${d}`))
  coreProcess.on('error', (err) => console.error('[core] failed to start:', err))
  coreProcess.on('exit', (code, signal) => {
    console.warn(`[core] exited (code=${code}, signal=${signal})`)
    coreProcess = null
  })
}

function stopCore() {
  coreProcess?.kill()
  coreProcess = null
}

function createWindow() {
  const iconPath = app.isPackaged
    ? path.join(process.resourcesPath, 'app-icon.png')
    : path.join(__dirname, '../public/app-icon.png')
  const win = new BrowserWindow({
    width: 1100,
    height: 740,
    minWidth: 820,
    minHeight: 520,
    backgroundColor: '#0e1116',
    title: 'SelfishNetwork',
    frame: false,
    titleBarStyle: process.platform === 'darwin' ? 'hiddenInset' : 'hidden',
    icon: iconPath,
    webPreferences: {
      preload: path.join(__dirname, 'preload.mjs'),
      // Renderer only talks to localhost over WS; no node integration needed.
      nodeIntegration: false,
      contextIsolation: true,
    },
  })

  ipcMain.on('window:minimize', () => {
    win.minimize()
  })
  ipcMain.on('window:maximize', () => {
    if (win.isMaximized()) {
      win.unmaximize()
    } else {
      win.maximize()
    }
  })
  ipcMain.on('window:close', () => {
    win.close()
  })

  if (DEV_URL) {
    win.loadURL(DEV_URL)
  } else {
    win.loadFile(path.join(__dirname, '../dist/index.html'))
  }
}

app.whenReady().then(() => {
  startCore()
  createWindow()
})

app.on('activate', () => {
  if (BrowserWindow.getAllWindows().length === 0) createWindow()
})

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit()
})

app.on('before-quit', stopCore)
