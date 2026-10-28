/// <reference types="vite/client" />

declare global {
  interface Window {
    selfishnet?: {
      version: string
      coreUrl: string
    }
  }
}

export {}
