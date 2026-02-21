<script lang="ts">
import type { ParseSource } from '../types';

const {
	source,
	parserUsed,
	warnings = [],
}: {
	source: ParseSource;
	parserUsed: string;
	warnings?: string[];
} = $props();

const sourceLabel = $derived.by(() => {
	if (source.kind === 'git') {
		return `${source.remote}@${source.ref}:${source.path}`;
	}
	if (source.kind === 'github') {
		return `${source.owner}/${source.repo}@${source.ref}:${source.path}`;
	}
	return source.filename;
});
</script>

<section class="mb-2 border border-border bg-bg-secondary px-3 py-2 text-xs">
	<div class="flex flex-wrap items-center gap-2 text-text-secondary">
		<span class="font-semibold text-text-primary">Source</span>
		<span>{source.kind}</span>
		<span>&middot;</span>
		<span class="break-all">{sourceLabel}</span>
		<span>&middot;</span>
		<span>parser: <span class="text-text-primary">{parserUsed}</span></span>
	</div>

	{#if warnings.length > 0}
		<ul class="mt-2 space-y-1 text-warning">
			{#each warnings as warning}
				<li>{warning}</li>
			{/each}
		</ul>
	{/if}
</section>
