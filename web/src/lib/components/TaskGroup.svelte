<script lang="ts">
	import type { Event } from '$lib/types';
	import { formatDuration } from '$lib/types';
	import EventView from './EventView.svelte';

	let { events, taskId }: { events: Event[]; taskId: string } = $props();

	let collapsed = $state(true);

	let taskTitle = $derived.by(() => {
		const startEvent = events.find((e) => e.event_type.type === 'TaskStart');
		if (startEvent && startEvent.event_type.type === 'TaskStart') {
			return startEvent.event_type.data.title ?? `Task ${taskId}`;
		}
		return `Task ${taskId}`;
	});

	let duration = $derived.by(() => {
		if (events.length < 2) return 0;
		const first = new Date(events[0].timestamp).getTime();
		const last = new Date(events[events.length - 1].timestamp).getTime();
		return Math.max(0, Math.floor((last - first) / 1000));
	});

	let innerEvents = $derived(
		events.filter((e) => e.event_type.type !== 'TaskStart' && e.event_type.type !== 'TaskEnd')
	);
</script>

<div class="my-2 rounded-lg border border-border bg-bg-secondary">
	<button
		onclick={() => (collapsed = !collapsed)}
		class="flex w-full items-center gap-2 rounded-t-lg px-3 py-2 text-left transition-colors hover:bg-bg-hover"
	>
		<span class="text-xs text-text-muted">{collapsed ? '+' : '-'}</span>
		<span class="flex h-5 w-5 items-center justify-center rounded bg-accent/20 text-xs font-bold text-accent">
			T
		</span>
		<span class="flex-1 truncate text-sm font-medium text-text-primary">{taskTitle}</span>
		<span class="text-xs text-text-muted">{events.length} events</span>
		{#if duration > 0}
			<span class="text-xs text-text-muted">{formatDuration(duration)}</span>
		{/if}
	</button>

	{#if !collapsed}
		<div class="border-t border-border px-2 py-1">
			{#each innerEvents as event (event.event_id)}
				<EventView {event} />
			{/each}
		</div>
	{/if}
</div>
