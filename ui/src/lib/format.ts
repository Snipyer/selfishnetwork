/** Human-readable bits/s (expects bits per second). */
export function fmtBps(bps: number): string {
  if (bps <= 0) return '0 bps'
  const units = ['bps', 'Kbps', 'Mbps', 'Gbps']
  let v = bps
  let i = 0
  while (v >= 1000 && i < units.length - 1) {
    v /= 1000
    i++
  }
  return `${v.toFixed(v < 10 && i > 0 ? 1 : 0)} ${units[i]}`
}

/** Human-readable byte rate (expects bits per second; shown as bytes/s). */
export function fmtByteRate(bps: number): string {
  if (bps <= 0) return '0 B/s'
  const units = ['B/s', 'KB/s', 'MB/s', 'GB/s']
  let v = bps / 8
  let i = 0
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i++
  }
  return `${v.toFixed(v < 10 && i > 0 ? 1 : 0)} ${units[i]}`
}

/** Human-readable byte totals. */
export function fmtBytes(bytes: number): string {
  if (bytes <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let v = bytes
  let i = 0
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i++
  }
  return `${v.toFixed(v < 10 && i > 0 ? 1 : 0)} ${units[i]}`
}
