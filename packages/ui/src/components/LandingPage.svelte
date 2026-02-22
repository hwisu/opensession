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
		id: 'data',
		title: 'Every Session Is Data',
		summary:
			'Messages, tool calls, diffs, corrections, and outcomes are stored as structured events.',
		proof: 'The same records are available in CLI, TUI, and web.',
	},
	{
		id: 'review',
		title: 'Review What Actually Happened',
		summary:
			'Use timeline view to check which tools ran, what changed, and where context shifted.',
		proof: 'Decisions can be verified from events, not memory.',
	},
	{
		id: 'share',
		title: 'Git-Native Sharing',
		summary:
			'Sessions are versioned, reviewed, and referenced by commit through normal git history.',
		proof: 'Sharing uses standard refs and regular repository workflows.',
	},
	{
		id: 'handoff',
		title: 'Reproducible Handoffs',
		summary:
			'Handoffs use the same session records, so the next owner can continue from the same state.',
		proof: 'No separate handoff summary is required as source of truth.',
	},
];

const operatingLoop = [
	{ label: 'Record', detail: 'Capture sessions as structured artifacts, not screenshots.' },
	{ label: 'Inspect', detail: 'Review timeline evidence to understand behavior and outcomes.' },
	{ label: 'Share', detail: 'Distribute reproducible artifacts through web and git refs.' },
	{ label: 'Refine', detail: 'Use findings to improve prompts, tools, and ownership transfer.' },
];
</script>

<svelte:head>
	<title>OpenSession - Version Control for AI Work</title>
</svelte:head>

<div class="landing-stage mx-auto w-full max-w-6xl px-3 py-8 sm:px-6 sm:py-10" data-testid="landing-page">
	<section class="hero-panel p-5 sm:p-6">
		<div class="grid gap-6 lg:grid-cols-[1.1fr_0.9fr] lg:gap-8">
			<div class="space-y-4">
				<p class="stage-kicker text-[11px] uppercase tracking-[0.18em] text-text-muted">version control for ai work</p>
				<h1 class="stage-title text-4xl leading-[0.92] text-text-primary sm:text-5xl lg:text-6xl">
					Version Control for AI Work.
				</h1>
				<p class="max-w-xl text-sm leading-relaxed text-text-secondary sm:text-base" data-testid="landing-hero-copy">
					AI coding sessions are easy to lose. Context disappears, handoffs degrade, and screenshots replace evidence.
					OpenSession turns AI sessions into structured, replayable artifacts.
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
					What That Means
				</p>
				<ul class="space-y-2">
					<li class="text-sm text-text-secondary">Timeline view instead of screenshot-based review.</li>
					<li class="text-sm text-text-secondary">Git-based sharing with commit-level references.</li>
					<li class="text-sm text-text-secondary">Handoffs tied to the same session records.</li>
				</ul>
				<div class="mt-4 grid gap-2 sm:grid-cols-2">
					<div class="signal-chip border border-border px-2 py-2 text-[11px] text-text-secondary">
						<span class="block text-text-muted">Session Model</span>
						<span class="block text-text-primary">Structured Events</span>
					</div>
					<div class="signal-chip border border-border px-2 py-2 text-[11px] text-text-secondary">
						<span class="block text-text-muted">Work Loop</span>
						<span class="block text-text-primary">Record -> Inspect -> Share -> Refine</span>
					</div>
				</div>
			</div>
		</div>
	</section>

	<section data-contract-section="goal-map" class="mise-panel mt-6 p-4 sm:p-5">
		<div class="mb-3 text-xs uppercase tracking-[0.12em] text-text-muted">$ goal-map</div>
		<h2 class="section-title mb-4 text-2xl text-text-primary sm:text-3xl">What This Means</h2>
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
		<h2 class="section-title mb-4 text-2xl text-text-primary sm:text-3xl">Work Loop</h2>
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
