<script lang="ts">
const {
	onNavigate = (_path: string) => {},
}: {
	onNavigate?: (path: string) => void;
} = $props();

// Typewriter for the hero terminal
const terminalLines = [
	{ type: 'prompt', text: '$ opensession upload ./session.jsonl' },
	{ type: 'output', text: 'Parsing HAIL session...' },
	{ type: 'output', text: '' },
	{
		type: 'meta',
		text: '{"v":"hail/0.1","tool":"claude-code","model":"opus-4","ts":"2026-02-11T09:14:00Z"}',
	},
	{
		type: 'human',
		text: '{"role":"human","content":"Fix the auth middleware to handle expired tokens"}',
	},
	{
		type: 'agent',
		text: '{"role":"agent","content":"I\'ll update the token validation...","tool_calls":3}',
	},
	{ type: 'tool', text: '{"type":"file_edit","path":"src/middleware/auth.rs","lines_changed":42}' },
	{
		type: 'agent',
		text: '{"role":"agent","content":"Fixed. The middleware now refreshes expired tokens automatically."}',
	},
	{ type: 'output', text: '' },
	{ type: 'success', text: 'Uploaded: session_a7f3e2 (8 events, 2.4k tokens, 3m 12s)' },
	{ type: 'success', text: 'Shared with team: engineering' },
];

let visibleLines = $state(0);
let cursorVisible = $state(true);

$effect(() => {
	const interval = setInterval(() => {
		if (visibleLines < terminalLines.length) {
			visibleLines++;
		} else {
			clearInterval(interval);
		}
	}, 280);
	return () => clearInterval(interval);
});

$effect(() => {
	const blink = setInterval(() => {
		cursorVisible = !cursorVisible;
	}, 530);
	return () => clearInterval(blink);
});

const tools = [
	{ name: 'Claude Code', cssColor: 'var(--color-tool-claude)', icon: 'CL' },
	{ name: 'Cursor', cssColor: 'var(--color-tool-cursor)', icon: 'CU' },
	{ name: 'Codex', cssColor: 'var(--color-tool-codex)', icon: 'CX' },
	{ name: 'OpenCode', cssColor: 'var(--color-tool-opencode)', icon: 'OC' },
	{ name: 'Cline', cssColor: 'var(--color-tool-cline)', icon: 'CI' },
	{ name: 'Amp', cssColor: 'var(--color-tool-amp)', icon: 'AM' },
	{ name: 'Gemini CLI', cssColor: 'var(--color-tool-gemini)', icon: 'GE' },
];

const features = [
	{
		flag: '--open',
		title: 'Open Format',
		desc: 'HAIL is a simple JSONL spec. No vendor lock-in. Your data, your rules.',
	},
	{
		flag: '--host',
		title: 'Self-Hostable',
		desc: 'One Docker command. Your sessions stay on your infrastructure.',
	},
	{
		flag: '--team',
		title: 'Team Sharing',
		desc: "Invite your team. Share sessions. Learn from each other's AI interactions.",
	},
	{
		flag: '--search',
		title: 'Searchable Archive',
		desc: 'Full-text search across all sessions. Find that solution you saw last week.',
	},
];

function getLineClass(type: string): string {
	switch (type) {
		case 'prompt':
			return 'text-accent';
		case 'meta':
			return 'text-text-muted';
		case 'human':
			return 'text-role-human';
		case 'agent':
			return 'text-role-agent';
		case 'tool':
			return 'text-role-tool';
		case 'success':
			return 'text-success';
		default:
			return 'text-text-secondary';
	}
}
</script>

<div class="landing-root relative overflow-x-hidden">
	<!-- Scanline overlay -->
	<div class="pointer-events-none fixed inset-0 scanline-overlay" style="z-index: var(--z-scanline)"></div>

	<!-- Hero -->
	<section class="relative overflow-hidden">
		<div class="mx-auto max-w-6xl px-6 pb-20 pt-16 md:pt-24">
			<div class="grid gap-10 lg:grid-cols-[1fr_1fr] lg:items-start lg:gap-16">
				<!-- Left: Copy -->
				<div class="flex flex-col justify-center">
					<div class="mb-4 inline-flex items-center gap-2 self-start border border-border px-3 py-1 text-xs uppercase tracking-[0.2em] text-text-muted">
						<span class="inline-block h-1.5 w-1.5 rounded-full bg-success shadow-[0_0_6px_var(--color-success)]"></span>
						Open Source
					</div>

					<h1 class="mb-5 text-3xl font-bold leading-tight tracking-tight text-text-primary md:text-4xl">
						AI sessions are<br/>
						<span class="text-accent">knowledge.</span><br/>
						Don't let them<br/>
						disappear.
					</h1>

					<p class="mb-8 max-w-md text-base leading-relaxed text-text-secondary">
						Every AI coding session holds solutions, patterns, and insights.
						OpenSession captures them in an open format so they can
						enrich the web — not vanish when you close your terminal.
					</p>

					<div class="flex items-center gap-3">
						<button
							onclick={() => onNavigate('/register')}
							class="group relative bg-accent px-6 py-2.5 text-sm font-bold text-white transition-all hover:shadow-[0_0_20px_rgba(59,130,246,0.3)]"
						>
							<span class="relative z-10">Get Started</span>
						</button>
						<a
							href="https://github.com/hwisu/opensession"
							target="_blank"
							rel="noopener"
							class="px-4 py-2.5 text-sm text-text-muted transition-colors hover:text-text-primary"
						>
							GitHub &rarr;
						</a>
					</div>
				</div>

				<!-- Right: Terminal -->
				<div class="terminal-window min-w-0 overflow-hidden border border-border lg:mt-[42px]">
					<!-- Title bar -->
					<div class="flex items-center gap-2 border-b border-border bg-bg-secondary px-4 py-2">
						<span class="h-2.5 w-2.5 rounded-full bg-error/60"></span>
						<span class="h-2.5 w-2.5 rounded-full bg-warning/60"></span>
						<span class="h-2.5 w-2.5 rounded-full bg-success/60"></span>
						<span class="ml-3 text-[11px] text-text-muted">terminal — opensession</span>
					</div>
					<!-- Content -->
					<div class="h-[220px] overflow-hidden bg-bg-primary p-4 text-[10px] leading-[1.7] lg:h-[276px] lg:text-[13px]">
						{#each terminalLines.slice(0, visibleLines) as line, i}
							<div class="truncate {getLineClass(line.type)}">
								{#if line.text === ''}
									<br/>
								{:else}
									{line.text}
								{/if}
							</div>
						{/each}
						{#if visibleLines < terminalLines.length}
							<span class="inline-block h-3.5 w-[7px] translate-y-[1px] {cursorVisible ? 'bg-accent' : 'bg-transparent'}"></span>
						{/if}
					</div>
				</div>
			</div>
		</div>
	</section>

	<!-- What is HAIL -->
	<section class="border-y border-border bg-bg-secondary/50">
		<div class="mx-auto max-w-6xl px-6 py-16 md:py-20">
			<div class="mb-3 text-xs uppercase tracking-[0.2em] text-text-muted">
				$ cat SPEC.md
			</div>
			<h2 class="mb-4 text-xl font-bold text-text-primary">
				HAIL — Human-AI Interaction Log
			</h2>
			<p class="mb-8 max-w-2xl text-base leading-relaxed text-text-secondary">
				A minimal, line-oriented JSONL format that captures everything:
				prompts, responses, tool calls, file edits, token counts, timing.
				One file per session. Human-readable. Git-friendly.
			</p>

			<div class="grid gap-px border border-border bg-border sm:grid-cols-3">
				<div class="bg-bg-primary p-5">
					<div class="mb-2 text-sm font-bold text-accent">Structured</div>
					<p class="text-xs leading-relaxed text-text-secondary">
						Every event is typed — messages, tool calls, file edits, errors.
						Query and filter with standard JSON tools.
					</p>
				</div>
				<div class="bg-bg-primary p-5">
					<div class="mb-2 text-sm font-bold text-accent">Complete</div>
					<p class="text-xs leading-relaxed text-text-secondary">
						Full context preserved. Token counts, timestamps, model info,
						tool metadata. Nothing is lost.
					</p>
				</div>
				<div class="bg-bg-primary p-5">
					<div class="mb-2 text-sm font-bold text-accent">Portable</div>
					<p class="text-xs leading-relaxed text-text-secondary">
						Plain JSONL. No proprietary format. Works with jq, grep,
						Python, or any language. Your data stays yours.
					</p>
				</div>
			</div>
		</div>
	</section>

	<!-- Features -->
	<section>
		<div class="mx-auto max-w-6xl px-6 py-16 md:py-20">
			<div class="mb-3 text-xs uppercase tracking-[0.2em] text-text-muted">
				$ opensession --help
			</div>
			<h2 class="mb-10 text-xl font-bold text-text-primary">
				Built for developers who share.
			</h2>

			<div class="grid gap-6 sm:grid-cols-2">
				{#each features as feat}
					<div class="group border border-border p-5 transition-colors hover:border-border-light">
						<div class="mb-3 flex items-center gap-2">
							<span class="text-xs text-accent">{feat.flag}</span>
						</div>
						<div class="mb-2 text-sm font-bold text-text-primary">{feat.title}</div>
						<p class="text-xs leading-relaxed text-text-secondary">{feat.desc}</p>
					</div>
				{/each}
			</div>
		</div>
	</section>

	<!-- Supported Tools -->
	<section class="border-y border-border bg-bg-secondary/50">
		<div class="mx-auto max-w-6xl px-6 py-16 md:py-20">
			<div class="mb-3 text-xs uppercase tracking-[0.2em] text-text-muted">
				$ opensession tools --list
			</div>
			<h2 class="mb-3 text-xl font-bold text-text-primary">
				Works with your tools.
			</h2>
			<p class="mb-10 max-w-lg text-base text-text-secondary">
				Import sessions from any supported AI coding assistant.
				More integrations coming.
			</p>

			<div class="flex flex-wrap gap-3">
				{#each tools as t}
					<div
						class="flex items-center gap-2.5 border border-border bg-bg-primary px-4 py-2.5 transition-all hover:border-border-light"
					>
						<span
							class="tui-badge shrink-0"
							class:tui-badge-tool={true}
						style="background-color: {t.cssColor}"
						>
							{t.icon}
						</span>
						<span class="text-sm text-text-primary">{t.name}</span>
					</div>
				{/each}
			</div>
		</div>
	</section>

	<!-- CTA -->
	<section>
		<div class="mx-auto max-w-6xl px-6 py-20 text-center md:py-28">
			<h2 class="mb-4 text-2xl font-bold text-text-primary md:text-3xl">
				Stop losing your AI sessions.
			</h2>
			<p class="mx-auto mb-8 max-w-lg text-base leading-relaxed text-text-secondary">
				Every conversation with AI is a piece of the puzzle.
				Share yours. Build the future together.
			</p>

			<div class="flex items-center justify-center gap-4">
				<button
					onclick={() => onNavigate('/register')}
					class="bg-accent px-8 py-3 text-sm font-bold text-white transition-all hover:shadow-[0_0_24px_rgba(59,130,246,0.35)]"
				>
					Create Account
				</button>
				<button
					onclick={() => onNavigate('/login')}
					class="border border-border px-6 py-3 text-sm text-text-secondary transition-colors hover:border-accent hover:text-accent"
				>
					Login
				</button>
			</div>

			<div class="mt-12 text-xs text-text-muted">
				<span>
					docker run -p 3000:3000 ghcr.io/hwisu/opensession
				</span>
			</div>
		</div>
	</section>

</div>

<style>
	.scanline-overlay {
		background: repeating-linear-gradient(
			0deg,
			transparent,
			transparent 2px,
			rgba(0, 0, 0, 0.015) 2px,
			rgba(0, 0, 0, 0.015) 4px
		);
	}

	.terminal-window {
		box-shadow:
			0 0 0 1px var(--color-border),
			0 20px 60px -15px rgba(0, 0, 0, 0.5);
	}
</style>
