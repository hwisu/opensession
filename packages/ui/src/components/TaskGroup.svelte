<script lang="ts">
import type { Event } from '../types';
import { formatDuration } from '../types';
import EventView from './EventView.svelte';

const { events, taskId }: { events: Event[]; taskId: string } = $props();

let collapsed = $state(true);

const taskTitle = $derived.by(() => {
	const startEvent = events.find((e) => e.event_type.type === 'TaskStart');
	if (startEvent && startEvent.event_type.type === 'TaskStart') {
		return startEvent.event_type.data.title ?? `Task ${taskId}`;
	}
	return `Task ${taskId}`;
});

const duration = $derived.by(() => {
	if (events.length < 2) return 0;
	const first = new Date(events[0].timestamp).getTime();
	const last = new Date(events[events.length - 1].timestamp).getTime();
	return Math.max(0, Math.floor((last - first) / 1000));
});

const innerEvents = $derived(
	events.filter((e) => e.event_type.type !== 'TaskStart' && e.event_type.type !== 'TaskEnd'),
);
</script>

<div class="my-1.5">
	<button
		onclick={() => (collapsed = !collapsed)}
		class="group flex w-full items-center gap-2 rounded-md border border-border bg-bg-secondary px-3 py-1.5 text-left text-xs transition-colors hover:bg-bg-hover"
	>
		<span class="text-text-muted transition-transform" class:rotate-90={!collapsed}>&rsaquo;</span>
		<span class="font-medium text-accent">T</span>
		<span class="flex-1 truncate font-medium text-text-primary">{taskTitle}</span>
		<span class="text-text-muted">{innerEvents.length} events</span>
		{#if duration > 0}
			<span class="font-mono text-[10px] text-text-muted">{formatDuration(duration)}</span>
		{/if}
	</button>

	{#if !collapsed}
		<div class="ml-4 mt-1 border-l border-border pl-2">
			{#each innerEvents as event (event.event_id)}
				<EventView {event} />
			{/each}
		</div>
	{/if}
</div>
