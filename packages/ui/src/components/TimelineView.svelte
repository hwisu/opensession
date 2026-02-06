<script lang="ts">
	import type { Event } from '../types';
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

	function jumpTo(msgNum: number) {
		if (msgNum < 0 || msgNum >= userMessageIndices.length) return;
		currentUserMsgIdx = msgNum;
		const targetIdx = userMessageIndices[msgNum];
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

	<!-- Main layout: message nav rail + timeline -->
	<div class="flex gap-0">
		<!-- Left: vertical user message nav rail -->
		{#if userMessageIndices.length > 1}
			<div class="mr-2 hidden sm:flex flex-col items-center shrink-0" style="width: 24px;">
				{#each userMessageIndices as _msgIdx, i}
					<button
						onclick={() => jumpTo(i)}
						class="relative flex h-6 w-6 items-center justify-center rounded-full text-[9px] font-bold transition-all
							{currentUserMsgIdx === i
							? 'bg-blue-500 text-white ring-2 ring-blue-400/50 scale-110'
							: 'bg-blue-500/20 text-blue-400 hover:bg-blue-500/40 hover:text-blue-300'}"
						title="Jump to user message {i + 1}"
					>
						{i + 1}
					</button>
					{#if i < userMessageIndices.length - 1}
						<div class="w-px flex-1 min-h-1.5 bg-border"></div>
					{/if}
				{/each}
			</div>
		{/if}

		<!-- Timeline content -->
		<div class="min-w-0 flex-1">
			{#each timeline as item, idx}
				<div data-timeline-idx={idx} class="rounded transition-all">
					<EventView event={item.event} />
				</div>
			{/each}
		</div>
	</div>
</div>
