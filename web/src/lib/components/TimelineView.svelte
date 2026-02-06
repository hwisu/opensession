<script lang="ts">
	import type { Event } from '$lib/types';
	import EventView from './EventView.svelte';

	let { events }: { events: Event[] } = $props();

	type TimelineItem = { kind: 'standalone'; event: Event };

	let viewMode = $state<'timeline' | 'messages'>('timeline');

	let timeline = $derived.by((): TimelineItem[] => {
		const items = events.map((e) => ({ kind: 'standalone' as const, event: e }));
		if (viewMode === 'messages') {
			return items.filter((item) =>
				['UserMessage', 'AgentMessage'].includes(item.event.event_type.type)
			);
		}
		return items;
	});

	// Track user message indices for jump
	let userMessageIndices = $derived.by(() => {
		const indices: number[] = [];
		timeline.forEach((item, i) => {
			if (item.kind === 'standalone' && item.event.event_type.type === 'UserMessage') {
				indices.push(i);
			}
		});
		return indices;
	});

	let currentUserMsgIdx = $state(-1);

	function jumpTo(direction: 'next' | 'prev' | 'exact', exactIdx?: number) {
		if (userMessageIndices.length === 0) return;
		if (direction === 'exact' && exactIdx !== undefined) {
			currentUserMsgIdx = exactIdx;
		} else if (direction === 'next') {
			currentUserMsgIdx = (currentUserMsgIdx + 1) % userMessageIndices.length;
		} else {
			currentUserMsgIdx =
				currentUserMsgIdx <= 0 ? userMessageIndices.length - 1 : currentUserMsgIdx - 1;
		}
		const targetIdx = userMessageIndices[currentUserMsgIdx];
		const el = document.querySelector(`[data-timeline-idx="${targetIdx}"]`);
		if (el) {
			el.scrollIntoView({ behavior: 'smooth', block: 'start' });
			el.classList.add('ring-2', 'ring-accent', 'ring-opacity-50');
			setTimeout(() => el.classList.remove('ring-2', 'ring-accent', 'ring-opacity-50'), 1500);
		}
	}
</script>

<div>
	<!-- Top controls -->
	<div class="mb-4 flex flex-wrap items-center gap-2">
		<div class="flex rounded-lg border border-border bg-bg-secondary p-0.5">
			{#each [
				{ mode: 'timeline', label: 'Timeline' },
				{ mode: 'messages', label: 'Messages Only' }
			] as btn}
				<button
					onclick={() => (viewMode = btn.mode as typeof viewMode)}
					class="rounded-md px-3 py-1.5 text-xs font-medium transition-colors
						{viewMode === btn.mode
						? 'bg-accent text-white'
						: 'text-text-secondary hover:text-text-primary'}"
				>
					{btn.label}
				</button>
			{/each}
		</div>

		<span class="text-xs text-text-muted">
			{timeline.length} items
			{#if userMessageIndices.length > 0}
				&middot; {userMessageIndices.length} user messages
			{/if}
		</span>
	</div>

	<!-- Jump to message bar -->
	{#if userMessageIndices.length > 1}
		<div class="mb-4 flex items-center gap-1 rounded-lg border border-border bg-bg-secondary px-3 py-2">
			{#if userMessageIndices.length <= 20}
				<!-- Numbered buttons for small sessions -->
				<span class="shrink-0 text-xs text-text-muted mr-1">Jump to</span>
				<div class="flex items-center gap-1 overflow-x-auto">
					{#each userMessageIndices as _msgIdx, i}
						<button
							onclick={() => jumpTo('exact', i)}
							class="shrink-0 rounded px-2 py-0.5 text-xs font-mono transition-colors
								{currentUserMsgIdx === i
								? 'bg-accent text-white'
								: 'text-text-secondary hover:bg-bg-hover hover:text-text-primary'}"
						>
							{i + 1}
						</button>
					{/each}
				</div>
			{:else}
				<!-- Compact nav for large sessions -->
				<button
					onclick={() => jumpTo('prev')}
					class="shrink-0 rounded px-2 py-0.5 text-xs text-text-secondary hover:bg-bg-hover hover:text-text-primary"
				>
					&#x25C0;
				</button>
				<span class="shrink-0 text-xs font-mono text-text-muted px-2">
					{currentUserMsgIdx >= 0 ? currentUserMsgIdx + 1 : '-'} / {userMessageIndices.length} messages
				</span>
				<button
					onclick={() => jumpTo('next')}
					class="shrink-0 rounded px-2 py-0.5 text-xs text-text-secondary hover:bg-bg-hover hover:text-text-primary"
				>
					&#x25B6;
				</button>
			{/if}
		</div>
	{/if}

	<!-- Timeline content -->
	<div>
		{#each timeline as item, idx}
			<div data-timeline-idx={idx} class="rounded transition-all">
				<EventView event={item.event} />
			</div>
		{/each}
	</div>
</div>
