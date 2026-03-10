<script lang="ts">
import { appLocale, translate } from '../i18n';

type FloatingJob = {
	id: string;
	label: string;
	detail?: string;
};

const {
	jobs = [],
}: {
	jobs?: FloatingJob[];
} = $props();
</script>

{#if jobs.length > 0}
	<div class="pointer-events-none fixed bottom-4 right-4 z-40 w-[min(92vw,22rem)] space-y-2">
		{#each jobs as job (job.id)}
			<div
				data-testid={'floating-job-' + job.id}
				class="pointer-events-auto rounded border border-border bg-bg-secondary/95 px-3 py-2 shadow-lg backdrop-blur-sm"
			>
				<div class="flex items-center gap-2">
					<span class="h-2 w-2 animate-pulse rounded-full bg-accent"></span>
					<p class="text-xs font-semibold text-text-primary">
						{job.id === 'session-refresh'
							? translate($appLocale, 'sessionList.refreshJobLabel')
							: job.label}
					</p>
				</div>
				{#if job.detail}
					<p class="mt-1 text-[11px] text-text-secondary">
						{job.id === 'session-refresh'
							? translate($appLocale, 'sessionList.refreshJobDetail')
							: job.detail}
					</p>
				{/if}
			</div>
		{/each}
	</div>
{/if}
