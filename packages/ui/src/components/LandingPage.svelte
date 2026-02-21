<script lang="ts">
const {
	onNavigate = (_path: string) => {},
}: {
	onNavigate?: (path: string) => void;
} = $props();

type GoalCard = {
	id: string;
	title: string;
	summary: string;
	proof: string;
};

const goalCards: GoalCard[] = [
	{
		id: 'capture',
		title: 'Capture Work As Data',
		summary:
			'Turn coding sessions into durable artifacts so every decision, handoff, and correction can be revisited.',
		proof: 'A shared HAIL model keeps CLI and web views aligned.',
	},
	{
		id: 'review',
		title: 'Review What Actually Happened',
		summary:
			'Inspect real timelines, tool usage, and outcomes instead of relying on memory or partial screenshots.',
		proof: 'Session feed and detail views focus on navigable evidence.',
	},
	{
		id: 'share',
		title: 'Share Reproducible Sessions',
		summary:
			'Git-based sharing means storing session artifacts on the opensession/sessions branch so they can be reviewed and replayed through standard git refs.',
		proof: 'Sharing remains transport-agnostic and auditable through normal git history.',
	},
	{
		id: 'improve',
		title: 'Improve Open Tooling Faster',
		summary:
			'Use high-signal public traces to compare workflows and improve prompts, agents, and model behavior.',
		proof: 'The product is optimized for learning loops, not vanity metrics.',
	},
];

const operatingLoop = [
	{ label: 'Record', detail: 'Capture session traces with enough structure to replay context.' },
	{ label: 'Inspect', detail: 'Read event timelines to verify what happened and why.' },
	{ label: 'Share', detail: 'Publish or git-share artifacts for reproducible review.' },
	{ label: 'Refine', detail: 'Feed findings back into prompts, tools, and process.' },
];
</script>

<svelte:head>
	<title>OpenSession - Engineered Session Intelligence</title>
</svelte:head>

<div class="landing-stage mx-auto w-full max-w-6xl px-3 py-8 sm:px-6 sm:py-10" data-testid="landing-page">
	<section class="hero-panel p-5 sm:p-6">
		<div class="grid gap-6 lg:grid-cols-[1.1fr_0.9fr] lg:gap-8">
			<div class="space-y-4">
				<p class="stage-kicker text-[11px] uppercase tracking-[0.18em] text-text-muted">owner's manual for ai sessions</p>
				<h1 class="stage-title text-4xl leading-[0.92] text-text-primary sm:text-5xl lg:text-6xl">
					Engineered for Repeatable Learning.
				</h1>
				<p class="max-w-xl text-sm leading-relaxed text-text-secondary sm:text-base" data-testid="landing-hero-copy">
					OpenSession is an online session-sharing site for AI coding work.
					Publish traces, review real timelines, and keep handoffs reproducible without losing fidelity.
				</p>
				<div class="flex flex-wrap gap-2">
					<button
						type="button"
						onclick={() => onNavigate('/sessions')}
						class="stage-cta bg-accent px-4 py-2 text-xs font-semibold text-white transition-colors hover:bg-accent/85"
					>
						Open Sessions
					</button>
					<button
						type="button"
						onclick={() => onNavigate('/docs')}
						class="stage-cta border border-border px-4 py-2 text-xs font-semibold text-text-secondary transition-colors hover:border-accent hover:text-accent"
					>
						Open Docs
					</button>
				</div>
			</div>

			<div class="stage-note border border-border bg-bg-primary/65 p-4">
				<p class="mb-3 text-xs font-semibold uppercase tracking-[0.12em] text-text-muted">
					Product Direction
				</p>
				<ul class="space-y-2">
					<li class="text-sm text-text-secondary">Readable timelines instead of black-box outputs.</li>
					<li class="text-sm text-text-secondary">CLI + TUI + web surfaces around one session model.</li>
					<li class="text-sm text-text-secondary">Handoff and git refs for reproducible ownership transfer.</li>
				</ul>
				<div class="mt-4 grid gap-2 sm:grid-cols-2">
					<div class="signal-chip border border-border px-2 py-2 text-[11px] text-text-secondary">
						<span class="block text-text-muted">Core Format</span>
						<span class="block text-text-primary">HAIL Sessions</span>
					</div>
					<div class="signal-chip border border-border px-2 py-2 text-[11px] text-text-secondary">
						<span class="block text-text-muted">Sharing Model</span>
						<span class="block text-text-primary">Web + Git Refs</span>
					</div>
				</div>
			</div>
		</div>
	</section>

	<section data-contract-section="goal-map" class="mise-panel mt-6 p-4 sm:p-5">
		<div class="mb-3 text-xs uppercase tracking-[0.12em] text-text-muted">$ goal-map</div>
		<h2 class="section-title mb-4 text-2xl text-text-primary sm:text-3xl">What We Optimize For</h2>
		<div class="grid gap-3 md:grid-cols-2">
			{#each goalCards as card}
				<article class="mise-card border border-border bg-bg-secondary/70 p-4" data-goal-id={card.id}>
					<div class="mb-2 flex items-center justify-between">
						<span class="text-xs text-accent">{card.id}</span>
						<span class="text-[11px] uppercase text-text-muted">Goal</span>
					</div>
					<h3 class="text-base font-semibold text-text-primary">{card.title}</h3>
					<p class="mt-2 text-xs leading-relaxed text-text-secondary">{card.summary}</p>
					<p class="mt-3 border-t border-border pt-2 text-xs text-text-muted">{card.proof}</p>
				</article>
			{/each}
		</div>
	</section>

	<section data-contract-section="operating-loop" class="mise-panel mt-6 p-4 sm:p-5">
		<div class="mb-3 text-xs uppercase tracking-[0.12em] text-text-muted">$ operating-loop</div>
		<h2 class="section-title mb-4 text-2xl text-text-primary sm:text-3xl">Operating Loop</h2>
		<div class="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
			{#each operatingLoop as step, idx}
				<div class="mise-card border border-border bg-bg-secondary/70 p-3" data-flow-step={step.label}>
					<div class="text-[11px] uppercase tracking-[0.08em] text-text-muted">Step {idx + 1}</div>
					<div class="mt-1 text-sm font-semibold text-text-primary">{step.label}</div>
					<p class="mt-2 text-xs leading-relaxed text-text-secondary">{step.detail}</p>
				</div>
			{/each}
		</div>
	</section>
</div>

<style>
	.landing-stage {
		position: relative;
	}

	.landing-stage::before {
		content: '';
		position: absolute;
		inset: 0;
		pointer-events: none;
		background:
			radial-gradient(85% 56% at 18% 4%, color-mix(in oklab, var(--color-accent) 16%, transparent), transparent),
			linear-gradient(180deg, transparent 0%, color-mix(in oklab, var(--color-bg-secondary) 38%, transparent) 100%);
		opacity: 0.32;
	}

	.hero-panel,
	.mise-panel {
		position: relative;
		border: 1px solid var(--color-border);
		background: color-mix(in oklab, var(--color-bg-secondary) 44%, transparent);
		box-shadow: 0 22px 66px color-mix(in oklab, var(--color-bg-primary) 82%, transparent);
	}

	.stage-title,
	.section-title {
		font-family: 'Iowan Old Style', 'Palatino Linotype', 'Book Antiqua', Palatino, serif;
		letter-spacing: -0.025em;
	}

	.stage-kicker {
		padding-left: 0.65rem;
		border-left: 1px solid var(--color-border-light);
	}

	.stage-note {
		box-shadow: inset 0 0 0 1px color-mix(in oklab, var(--color-border) 72%, transparent);
	}

	.mise-card {
		position: relative;
		box-shadow: inset 0 0 0 1px color-mix(in oklab, var(--color-border) 72%, transparent);
	}

	.mise-card::before {
		content: '';
		position: absolute;
		top: 0;
		left: 0;
		width: 100%;
		height: 1px;
		background: color-mix(in oklab, #ff4e4e 72%, var(--color-border-light));
	}

	.stage-cta {
		letter-spacing: 0.015em;
	}
</style>
