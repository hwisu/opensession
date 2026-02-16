<script lang="ts">
	import type { Event } from '../types';
	import { isBoilerplateEvent, isToolError, pairToolCallResults } from '../event-helpers';
	import EventView from './EventView.svelte';
	import ThreadMinimap from './ThreadMinimap.svelte';

	const { events }: { events: Event[] } = $props();

	type TimelineItem = { event: Event; pairedResult?: Event; resultOk?: boolean };

	let viewMode = $state<'timeline' | 'messages'>('timeline');

	const filters = $state({
		messages: true,
		toolCalls: true,
		thinking: true,
		fileOps: true,
		shell: true,
	});

	const filterCategories: { key: keyof typeof filters; label: string }[] = [
		{ key: 'messages', label: 'Messages' },
		{ key: 'toolCalls', label: 'Tool Calls' },
		{ key: 'thinking', label: 'Thinking' },
		{ key: 'fileOps', label: 'File Ops' },
		{ key: 'shell', label: 'Shell' },
	];

	function matchesFilter(eventTypeName: string): boolean {
		if (['UserMessage', 'AgentMessage', 'SystemMessage'].includes(eventTypeName)) return filters.messages;
		if (['ToolCall', 'ToolResult'].includes(eventTypeName)) return filters.toolCalls;
		if (eventTypeName === 'Thinking') return filters.thinking;
		if (
			['FileRead', 'FileEdit', 'FileCreate', 'FileDelete', 'FileSearch', 'CodeSearch'].includes(
				eventTypeName,
			)
		)
			return filters.fileOps;
		if (eventTypeName === 'ShellCommand') return filters.shell;
		return true;
	}

	function getRoleGroup(event: Event): 'user' | 'agent' {
		if (event.event_type.type === 'UserMessage') return 'user';
		return 'agent';
	}

	const timeline = $derived.by((): TimelineItem[] => {
		if (viewMode === 'messages') {
			return events
				.filter((event) => ['UserMessage', 'AgentMessage'].includes(event.event_type.type))
				.map((event) => ({ event }));
		}
		const filtered = events.filter(
			(event) => matchesFilter(event.event_type.type) && !isBoilerplateEvent(event),
		);
		const pairs = pairToolCallResults(filtered);
		return filtered.map((event, idx) => {
			const paired = pairs.get(idx);
			return {
				event,
				pairedResult: paired,
				resultOk: paired ? !isToolError(paired.event_type) : false,
			};
		});
	});

	const userMessageCount = $derived(events.filter((e) => e.event_type.type === 'UserMessage').length);

	function handleFilterKeydown(e: KeyboardEvent) {
		if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
		if (viewMode !== 'timeline') return;
		const num = parseInt(e.key, 10);
		if (num >= 1 && num <= filterCategories.length) {
			e.preventDefault();
			const cat = filterCategories[num - 1];
			filters[cat.key] = !filters[cat.key];
		}
	}
</script>

<svelte:window onkeydown={handleFilterKeydown} />

{#snippet roleSeparator(currentRole: string)}
	<div class="my-1 sm:my-6 flex items-center gap-3">
		<div class="h-px flex-1 bg-border/50"></div>
		<span class="text-[10px] font-medium uppercase tracking-wider text-text-muted">{currentRole}</span>
		<div class="h-px flex-1 bg-border/50"></div>
	</div>
{/snippet}

<ThreadMinimap {events} />

<div>
	<div class="mb-4 flex flex-wrap items-center gap-2">
		<div class="flex items-center gap-1" role="tablist" aria-label="View mode">
			{#each [
				{ mode: 'timeline', label: 'Timeline' },
				{ mode: 'messages', label: 'Messages' }
			] as btn}
				<button
					role="tab"
					aria-selected={viewMode === btn.mode}
					onclick={() => (viewMode = btn.mode as typeof viewMode)}
					class="px-2 py-0.5 text-xs font-medium transition-colors
						{viewMode === btn.mode
						? 'text-accent'
						: 'text-text-secondary hover:text-text-primary'}"
				>
					{viewMode === btn.mode ? `[${btn.label}]` : btn.label}
				</button>
			{/each}
		</div>

		{#if viewMode === 'timeline'}
			<div class="flex flex-wrap items-center gap-1" role="group" aria-label="Event filters">
				{#each filterCategories as cat, i}
					<button
						aria-pressed={filters[cat.key]}
						onclick={() => (filters[cat.key] = !filters[cat.key])}
						class="px-2 py-0.5 text-[11px] font-medium transition-colors
							{filters[cat.key]
							? 'text-accent'
							: 'text-text-muted hover:text-text-secondary'}"
					>
						{i + 1}:{cat.label}
					</button>
				{/each}
			</div>
		{/if}

		<span class="text-xs text-text-muted">
			{timeline.length} items
			{#if userMessageCount > 0}
				&middot; {userMessageCount} user messages
			{/if}
		</span>
	</div>

	<div class="overflow-x-auto pb-2">
		<div>
			{#each timeline as item, idx}
				{@const currentRole = getRoleGroup(item.event)}
				{@const previousRole = idx > 0 ? getRoleGroup(timeline[idx - 1].event) : null}
				{#if previousRole && previousRole !== currentRole}
					{@render roleSeparator(currentRole)}
				{/if}
				<div data-timeline-idx={idx} class="transition-all">
					<EventView event={item.event} pairedResult={item.pairedResult} resultOk={item.resultOk} />
				</div>
			{/each}
		</div>
	</div>
</div>
