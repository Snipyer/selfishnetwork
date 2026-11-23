import { Button } from './ui/button'
import { shell } from '../lib/shell'
import { GITHUB_URL, KOFI_URL } from '../lib/links'
import { useSupportPrompt, type PromptId } from '../lib/useSupportPrompt'
import githubIcon from '../assets/github.png'
import kofiIcon from '../assets/kofi.webp'

const COPY: Record<PromptId, { icon: string; iconClass?: string; title: string; body: string; cta: string; url: string }> = {
	star: {
		icon: githubIcon,
		title: 'Enjoying SelfishNetwork?',
		body: 'A star on GitHub helps other people find it.',
		cta: 'Star on GitHub',
		url: GITHUB_URL,
	},
	support: {
		icon: kofiIcon,
		iconClass: 'kofi-icon',
		title: 'Support development',
		body: 'SelfishNetwork is free. If it’s useful to you, consider buying me a coffee.',
		cta: 'Support me',
		url: KOFI_URL,
	},
}

/** Bottom-centre nudge asking for a GitHub star or a Ko-fi tip. */
export function SupportBanner() {
	const { active, accept, remindLater, decline } = useSupportPrompt()
	if (!active) return null
	const c = COPY[active]

	return (
		<div
			role="dialog"
			aria-labelledby="support-banner-title"
			className="support-banner fixed inset-x-0 bottom-4 z-50 mx-auto w-[min(36rem,calc(100vw-2rem))] border border-border bg-card text-card-foreground shadow-lg"
		>
			<div className="flex items-start gap-3 p-4">
				<img src={c.icon} alt="" className={`mt-0.5 size-8 shrink-0 object-contain ${c.iconClass ?? ''}`} />
				<div className="min-w-0 flex-1">
					<p id="support-banner-title" className="text-sm font-medium">
						{c.title}
					</p>
					<p className="mt-0.5 text-xs text-muted-foreground">{c.body}</p>
					<div className="mt-3 flex flex-wrap items-center gap-2">
						<Button
							size="sm"
							onClick={() => {
								shell.openExternal(c.url)
								accept()
							}}
						>
							{c.cta}
						</Button>
						<Button size="sm" variant="outline" onClick={remindLater}>
							Remind me later
						</Button>
						<Button size="sm" variant="ghost" onClick={decline}>
							No thanks
						</Button>
					</div>
				</div>
			</div>
		</div>
	)
}
