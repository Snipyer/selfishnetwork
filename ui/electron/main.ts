import { app, BrowserWindow } from 'electron'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))

// vite-plugin-electron sets this in dev; in a packaged build we load the file.
const DEV_URL = process.env.VITE_DEV_SERVER_URL

function createWindow() {
  const win = new BrowserWindow({
    width: 1100,
    height: 740,
    minWidth: 820,
    minHeight: 520,
    backgroundColor: '#0e1116',
    title: 'SelfishNet',
    webPreferences: {
      preload: path.join(__dirname, 'preload.mjs'),
      // Renderer only talks to localhost over WS; no node integration needed.
      nodeIntegration: false,
      contextIsolation: true,
    },
  })

  if (DEV_URL) {
    win.loadURL(DEV_URL)
  } else {
    win.loadFile(path.join(__dirname, '../dist/index.html'))
  }
}

app.whenReady().then(createWindow)

app.on('activate', () => {
  if (BrowserWindow.getAllWindows().length === 0) createWindow()
})

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit()
})
