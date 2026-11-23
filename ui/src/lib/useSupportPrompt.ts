import { useEffect, useState } from 'react'

// Nudges shown in the bottom banner. Each one is shown at most once per
// session, only after it becomes due, and never again once the user acts on it
// or says "No thanks". "Remind me later" pushes it back by REMIND_AFTER.

export type PromptId = 'star' | 'support'

type PromptState = { status: 'pending' | 'done'; dueAt: number }
type Stored = Record<PromptId, PromptState>

const STORAGE_KEY = 'selfishnet-support-prompts'
const DAY = 24 * 60 * 60 * 1000
const REMIND_AFTER = 1 * DAY
// Delay after launch before a due prompt slides in, so it never greets the user
// on top of the initial scan.
const SHOW_DELAY_MS = 20_000

// First-run schedule: ask for a star after a day of use, support a few days later.
// Order here is also priority order when both are due.
const FIRST_DUE: Record<PromptId, number> = { star: 1 * DAY, support: 5 * DAY }

function load(): Stored {
	const now = Date.now()
	const fresh: Stored = {
		star: { status: 'pending', dueAt: now + FIRST_DUE.star },
		support: { status: 'pending', dueAt: now + FIRST_DUE.support },
	}
	try {
		const raw = localStorage.getItem(STORAGE_KEY)
		if (raw) return { ...fresh, ...(JSON.parse(raw) as Partial<Stored>) }
	} catch {
		// Corrupt or unavailable storage: fall through to a fresh schedule.
	}
	save(fresh)
	return fresh
}

function save(state: Stored) {
	try {
		localStorage.setItem(STORAGE_KEY, JSON.stringify(state))
	} catch {
		// Non-fatal: the prompt just may reappear next launch.
	}
}

function duePrompt(state: Stored): PromptId | null {
	const now = Date.now()
	for (const id of Object.keys(FIRST_DUE) as PromptId[]) {
		const p = state[id]
		if (p.status === 'pending' && p.dueAt <= now) return id
	}
	return null
}

export function useSupportPrompt() {
	const [active, setActive] = useState<PromptId | null>(null)

	useEffect(() => {
		const id = duePrompt(load())
		if (!id) return
		const t = window.setTimeout(() => setActive(id), SHOW_DELAY_MS)
		return () => window.clearTimeout(t)
	}, [])

	const update = (patch: PromptState) => {
		if (!active) return
		const state = load()
		state[active] = patch
		save(state)
		setActive(null)
	}

	return {
		active,
		/** User starred / supported — never ask again. */
		accept: () => update({ status: 'done', dueAt: 0 }),
		remindLater: () => update({ status: 'pending', dueAt: Date.now() + REMIND_AFTER }),
		/** "No thanks" — never ask again. */
		decline: () => update({ status: 'done', dueAt: 0 }),
	}
}
