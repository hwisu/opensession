<script lang="ts">
import { onDestroy, onMount } from 'svelte';
import type { Event } from '../types';

const { events }: { events: Event[] } = $props();

let activeIdx = $state(0);
let observer: IntersectionObserver | null = null;

const userMessageIndices = $derived.by(() => {
	const indices: number[] = [];
	events.forEach((e, i) => {
		if (e.event_type.type === 'UserMessage') indices.push(i);
	});
	return indices;
});

function scrollToMessage(msgNum: number) {
	const timelineIdx = userMessageIndices[msgNum];
	if (timelineIdx == null) return;
	const el = document.querySelector(`[data-timeline-idx="${timelineIdx}"]`);
	if (el) {
		el.scrollIntoView({ behavior: 'smooth', block: 'start' });
	}
}

onMount(() => {
	observer = new IntersectionObserver(
		(entries) => {
			for (const entry of entries) {
				if (entry.isIntersecting) {
					const idx = Number((entry.target as HTMLElement).dataset.timelineIdx);
					const msgNum = userMessageIndices.indexOf(idx);
					if (msgNum >= 0) activeIdx = msgNum;
				}
			}
		},
		{ rootMargin: '-20% 0px -60% 0px', threshold: 0 },
	);

	// Observe all user message elements
	for (const timelineIdx of userMessageIndices) {
		const el = document.querySelector(`[data-timeline-idx="${timelineIdx}"]`);
		if (el) observer.observe(el);
	}
});

onDestroy(() => {
	observer?.disconnect();
});
</script>

{#if userMessageIndices.length > 1}
	<nav class="fixed left-4 top-1/2 -translate-y-1/2 hidden lg:flex flex-col items-center gap-1" style="z-index: var(--z-minimap)" aria-label="Message navigation">
		{#each userMessageIndices as _, i}
			<button
				onclick={() => scrollToMessage(i)}
				class="group relative flex items-center"
				title="Message {i + 1}"
			>
				<!-- Square -->
				<span
					class="block h-2 w-2 transition-all
						{activeIdx === i
						? 'bg-accent scale-125'
						: 'bg-border-light opacity-50 group-hover:opacity-100 group-hover:bg-text-muted'}"
				></span>
				<!-- Tooltip -->
				<span class="absolute left-5 hidden group-hover:block whitespace-nowrap bg-bg-tertiary px-2 py-0.5 text-[10px] text-text-secondary shadow-lg border border-border">
					{i + 1}
				</span>
			</button>
			<!-- Connector line between dots (except after last) -->
			{#if i < userMessageIndices.length - 1}
				<span class="h-2 w-px bg-border-light opacity-30"></span>
			{/if}
		{/each}
	</nav>
{/if}
