import { contextBridge } from 'electron'

// The renderer connects to the core directly over a localhost WebSocket, so the
// preload surface is intentionally tiny — just app metadata for now.
contextBridge.exposeInMainWorld('selfishnet', {
  version: '0.1.0',
  coreUrl: 'ws://127.0.0.1:8734',
})
