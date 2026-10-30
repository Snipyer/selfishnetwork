import { useEffect, useState } from 'react'
import { shell } from './shell'

// Where the released builds live. The "latest" release page is what we point
// the user at; the API endpoint beside it tells us the newest published tag.
const REPO = 'Snipyer/selfishnetwork'
export const RELEASES_URL = `https://github.com/${REPO}/releases/latest`
const LATEST_API = `https://api.github.com/repos/${REPO}/releases/latest`

/** Parse a tag like "v0.1.1" or "0.1.1" into numeric [major, minor, patch]. */
function parseVersion(raw: string): number[] {
  return raw
    .trim()
    .replace(/^v/i, '')
    .split('.')
    .map((n) => parseInt(n, 10) || 0)
}

/** True when `latest` is a strictly newer semver than `current`. */
function isNewer(latest: string, current: string): boolean {
  const a = parseVersion(latest)
  const b = parseVersion(current)
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const diff = (a[i] ?? 0) - (b[i] ?? 0)
    if (diff !== 0) return diff > 0
  }
  return false
}

/**
 * Checks the GitHub releases for a version newer than the running build.
 * Returns the newer version string once found, or null while unknown / current.
 * Network or API failures are swallowed — a missed check just means no banner.
 */
export function useUpdateCheck(): { latestVersion: string | null; releaseUrl: string } {
  const [latestVersion, setLatestVersion] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false

    async function check() {
      try {
        const current = await shell.version()
        const res = await fetch(LATEST_API, {
          headers: { Accept: 'application/vnd.github+json' },
        })
        if (!res.ok) return
        const data: { tag_name?: string } = await res.json()
        const tag = data.tag_name
        if (!cancelled && tag && isNewer(tag, current)) {
          setLatestVersion(tag.replace(/^v/i, ''))
        }
      } catch {
        // Offline or rate-limited — leave the banner hidden.
      }
    }

    check()
    return () => {
      cancelled = true
    }
  }, [])

  return { latestVersion, releaseUrl: RELEASES_URL }
}
