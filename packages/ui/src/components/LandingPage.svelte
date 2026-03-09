<script lang="ts">
import { appLocale, translate } from '../i18n';

const {
	onNavigate = (_path: string) => {},
}: {
	onNavigate?: (path: string) => void;
} = $props();

type DesktopWindow = Window & {
	__TAURI_INTERNALS__?: unknown;
	__OPENSESSION_DESKTOP_COMPACT_LANDING__?: boolean;
};

type GoalCard = {
	id: string;
	title: string;
	summary: string;
	proof: string;
};

const goalCards = $derived.by((): GoalCard[] => [
	{
		id: 'data',
		title: translate($appLocale, 'landing.goal.data.title'),
		summary: translate($appLocale, 'landing.goal.data.summary'),
		proof: translate($appLocale, 'landing.goal.data.proof'),
	},
	{
		id: 'review',
		title: translate($appLocale, 'landing.goal.review.title'),
		summary: translate($appLocale, 'landing.goal.review.summary'),
		proof: translate($appLocale, 'landing.goal.review.proof'),
	},
	{
		id: 'share',
		title: translate($appLocale, 'landing.goal.share.title'),
		summary: translate($appLocale, 'landing.goal.share.summary'),
		proof: translate($appLocale, 'landing.goal.share.proof'),
	},
	{
		id: 'handoff',
		title: translate($appLocale, 'landing.goal.handoff.title'),
		summary: translate($appLocale, 'landing.goal.handoff.summary'),
		proof: translate($appLocale, 'landing.goal.handoff.proof'),
	},
]);

const operatingLoop = $derived.by(() => [
	{
		label: translate($appLocale, 'landing.loop.record.label'),
		detail: translate($appLocale, 'landing.loop.record.detail'),
	},
	{
		label: translate($appLocale, 'landing.loop.inspect.label'),
		detail: translate($appLocale, 'landing.loop.inspect.detail'),
	},
	{
		label: translate($appLocale, 'landing.loop.share.label'),
		detail: translate($appLocale, 'landing.loop.share.detail'),
	},
	{
		label: translate($appLocale, 'landing.loop.refine.label'),
		detail: translate($appLocale, 'landing.loop.refine.detail'),
	},
]);

let compactDesktopLanding = $state(false);

$effect(() => {
	if (typeof window === 'undefined') return;
	const desktopWindow = window as DesktopWindow;
	const forced = desktopWindow.__OPENSESSION_DESKTOP_COMPACT_LANDING__;
	if (typeof forced === 'boolean') {
		compactDesktopLanding = forced;
		return;
	}
	compactDesktopLanding =
		'__TAURI_INTERNALS__' in desktopWindow || desktopWindow.location.protocol === 'tauri:';
});
</script>

<svelte:head>
	<title>{translate($appLocale, 'landing.title')}</title>
</svelte:head>

<div class="landing-stage mx-auto w-full max-w-6xl px-3 py-8 sm:px-6 sm:py-10" data-testid="landing-page">
	<section class="hero-panel p-5 sm:p-6">
		<div class="grid gap-6 lg:grid-cols-[1.1fr_0.9fr] lg:gap-8">
			<div class="space-y-4">
				<p class="stage-kicker text-[11px] uppercase tracking-[0.18em] text-text-muted">
					{translate($appLocale, 'landing.kicker')}
				</p>
				<h1 class="stage-title text-4xl leading-[0.92] text-text-primary sm:text-5xl lg:text-6xl">
					{translate($appLocale, 'landing.heroTitle')}
				</h1>
				<p class="max-w-xl text-sm leading-relaxed text-text-secondary sm:text-base" data-testid="landing-hero-copy">
					{translate($appLocale, 'landing.heroCopy')}
				</p>
					<div class="flex flex-wrap gap-2">
						<button
							type="button"
							onclick={() => onNavigate('/sessions')}
							class="stage-cta bg-accent px-4 py-2 text-xs font-semibold text-white transition-colors hover:bg-accent/85"
						>
							{translate($appLocale, 'landing.openSessions')}
						</button>
						<button
							type="button"
							onclick={() => onNavigate('/docs#getting-started')}
							class="stage-cta bg-warning px-4 py-2 text-xs font-semibold text-text-primary transition-colors hover:bg-warning/85"
						>
							{translate($appLocale, 'landing.quickStart')}
						</button>
						<button
							type="button"
							onclick={() => onNavigate('/docs')}
							class="stage-cta border border-border px-4 py-2 text-xs font-semibold text-text-secondary transition-colors hover:border-accent hover:text-accent"
						>
						{translate($appLocale, 'landing.openDocs')}
					</button>
				</div>
			</div>

			<div class="stage-note border border-border bg-bg-primary/65 p-4">
				<p class="mb-3 text-xs font-semibold uppercase tracking-[0.12em] text-text-muted">
					{translate($appLocale, 'landing.whatMeans')}
				</p>
				<ul class="space-y-2">
					<li class="text-sm text-text-secondary">{translate($appLocale, 'landing.timelineReview')}</li>
					<li class="text-sm text-text-secondary">{translate($appLocale, 'landing.gitSharing')}</li>
					<li class="text-sm text-text-secondary">{translate($appLocale, 'landing.handoffs')}</li>
				</ul>
				<div class="mt-4 grid gap-2 sm:grid-cols-2">
					<div class="signal-chip border border-border px-2 py-2 text-[11px] text-text-secondary">
						<span class="block text-text-muted">{translate($appLocale, 'landing.sessionModel')}</span>
						<span class="block text-text-primary">{translate($appLocale, 'landing.structuredEvents')}</span>
					</div>
					<div class="signal-chip border border-border px-2 py-2 text-[11px] text-text-secondary">
						<span class="block text-text-muted">{translate($appLocale, 'landing.workLoop')}</span>
						<span class="block text-text-primary">{translate($appLocale, 'landing.workLoopValue')}</span>
					</div>
				</div>
			</div>
		</div>
	</section>

	{#if !compactDesktopLanding}
		<section data-contract-section="goal-map" class="mise-panel mt-6 p-4 sm:p-5">
			<div class="mb-3 text-xs uppercase tracking-[0.12em] text-text-muted">
				{translate($appLocale, 'landing.goalMapTag')}
			</div>
			<h2 class="section-title mb-4 text-2xl text-text-primary sm:text-3xl">
				{translate($appLocale, 'landing.goalMapTitle')}
			</h2>
			<div class="grid gap-3 md:grid-cols-2">
				{#each goalCards as card}
					<article class="mise-card border border-border bg-bg-secondary/70 p-4" data-goal-id={card.id}>
						<div class="mb-2 flex items-center justify-between">
							<span class="text-xs text-accent">{card.id}</span>
							<span class="text-[11px] uppercase text-text-muted">
								{translate($appLocale, 'landing.goalLabel')}
							</span>
						</div>
						<h3 class="text-base font-semibold text-text-primary">{card.title}</h3>
						<p class="mt-2 text-xs leading-relaxed text-text-secondary">{card.summary}</p>
						<p class="mt-3 border-t border-border pt-2 text-xs text-text-muted">{card.proof}</p>
					</article>
				{/each}
			</div>
		</section>

		<section data-contract-section="operating-loop" class="mise-panel mt-6 p-4 sm:p-5">
			<div class="mb-3 text-xs uppercase tracking-[0.12em] text-text-muted">
				{translate($appLocale, 'landing.operatingLoopTag')}
			</div>
			<h2 class="section-title mb-4 text-2xl text-text-primary sm:text-3xl">
				{translate($appLocale, 'landing.operatingLoopTitle')}
			</h2>
			<div class="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
				{#each operatingLoop as step, idx}
					<div class="mise-card border border-border bg-bg-secondary/70 p-3" data-flow-step={step.label}>
						<div class="text-[11px] uppercase tracking-[0.08em] text-text-muted">
							{translate($appLocale, 'landing.step', { number: idx + 1 })}
						</div>
						<div class="mt-1 text-sm font-semibold text-text-primary">{step.label}</div>
						<p class="mt-2 text-xs leading-relaxed text-text-secondary">{step.detail}</p>
					</div>
				{/each}
			</div>
		</section>
	{/if}
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
