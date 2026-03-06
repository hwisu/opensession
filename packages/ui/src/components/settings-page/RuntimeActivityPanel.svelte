<script lang="ts">
import type { RuntimeActivityCard, RuntimeActivityTone } from './models';

const {
	cards = [],
}: {
	cards?: RuntimeActivityCard[];
} = $props();

function activityPillClasses(tone: RuntimeActivityTone): string {
	if (tone === 'running') {
		return 'border-accent/40 bg-accent/5 text-accent';
	}
	if (tone === 'enabled') {
		return 'border-emerald-500/40 bg-emerald-500/10 text-emerald-700';
	}
	if (tone === 'failed') {
		return 'border-error/40 bg-error/10 text-error';
	}
	if (tone === 'complete') {
		return 'border-sky-500/40 bg-sky-500/10 text-sky-700';
	}
	return 'border-border/70 bg-bg-primary text-text-secondary';
}
</script>

<section
	id="runtime-section-activity"
	class="scroll-mt-24 space-y-3 border border-border/60 p-3"
	data-testid="settings-runtime-activity"
>
	<div class="flex flex-wrap items-start justify-between gap-2">
		<div>
			<h3 class="text-xs font-semibold uppercase tracking-[0.08em] text-text-muted">
				Background Activity
			</h3>
			<p class="mt-1 text-[11px] text-text-secondary">
				Live view of desktop auto jobs, cleanup loops, and their last recorded work.
			</p>
		</div>
		<p class="text-[11px] text-text-muted">
			Updates while this page stays open.
		</p>
	</div>
	<div class="grid gap-3 lg:grid-cols-3">
		{#each cards as card}
			<article
				class="space-y-2 rounded border border-border/60 bg-bg-primary px-3 py-3"
				data-testid={card.testId}
			>
				<div class="flex flex-wrap items-start justify-between gap-2">
					<div>
						<p class="text-sm font-semibold text-text-primary">{card.title}</p>
						<p class="mt-1 text-[11px] text-text-secondary">{card.subtitle}</p>
					</div>
					<div class="flex flex-wrap gap-1">
						{#each card.badges as badge}
							<span
								class={`inline-flex items-center border px-2 py-1 text-[11px] font-semibold ${activityPillClasses(badge.tone)}`}
							>
								{badge.label}
							</span>
						{/each}
					</div>
				</div>
				{#each card.lines as line}
					<p class="text-[11px] text-text-secondary">{line}</p>
				{/each}
				<p class="text-[11px] text-text-muted">{card.timestampLine}</p>
			</article>
		{/each}
	</div>
</section>
