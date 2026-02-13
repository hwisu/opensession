<script lang="ts">
import type { Snippet } from 'svelte';
import { chevronRightIcon } from './icons';

let {
	icon,
	label,
	expanded = $bindable(false),
	hasContent = false,
	nameLabel = '',
	nameColorClass = 'text-text-muted',
	metaBadge,
	children,
}: {
	icon: string;
	label: string;
	expanded?: boolean;
	hasContent?: boolean;
	nameLabel?: string;
	nameColorClass?: string;
	metaBadge?: Snippet;
	children?: Snippet;
} = $props();
</script>

<div class="ev-chip my-0.5">
	<button
		onclick={() => (expanded = !expanded)}
		aria-expanded={hasContent ? expanded : undefined}
		class="group flex w-full items-center gap-2 border border-transparent bg-transparent px-3 py-1.5 text-left text-xs transition-colors hover:bg-bg-hover"
	>
		<span class="shrink-0 inline-flex text-text-muted">{@html icon}</span>
		{#if nameLabel}
			<span class="shrink-0 font-medium {nameColorClass}">{nameLabel}</span>
		{/if}
		<span class="min-w-0 flex-1 truncate font-mono text-text-secondary group-hover:text-text-primary">{label}</span>
		{#if metaBadge}
			{@render metaBadge()}
		{/if}
		{#if hasContent}
			<span class="shrink-0 inline-flex text-text-muted transition-transform" class:rotate-90={expanded}>{@html chevronRightIcon}</span>
		{/if}
	</button>

	{#if expanded && hasContent && children}
		<div class="ml-4 mt-1 overflow-hidden border-l-2 border-l-text-muted bg-bg-primary text-xs">
			{@render children()}
		</div>
	{/if}
</div>
