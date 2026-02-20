<script lang="ts">
import { isBoilerplateEvent, isToolError, pairToolCallResults } from '../event-helpers';
import {
	buildNativeFilterOptions,
	buildUnifiedFilterOptions,
	filterEventsByNativeGroups,
	filterEventsByUnifiedKeys,
	type FilterOption,
	type SessionViewMode,
} from '../session-filters';
import type { Event } from '../types';
import EventView from './EventView.svelte';
import ThreadMinimap from './ThreadMinimap.svelte';

const {
	events,
	viewMode = 'unified',
	nativeEnabled = false,
	unifiedFilters = new Set<string>(),
	nativeFilters = new Set<string>(),
	onViewModeChange = (_mode: SessionViewMode) => {},
	onToggleUnifiedFilter = (_key: string) => {},
	onToggleNativeFilter = (_key: string) => {},
	nativeAdapter = null,
}: {
	events: Event[];
	viewMode?: SessionViewMode;
	nativeEnabled?: boolean;
	unifiedFilters?: Set<string>;
	nativeFilters?: Set<string>;
	onViewModeChange?: (mode: SessionViewMode) => void;
	onToggleUnifiedFilter?: (key: string) => void;
	onToggleNativeFilter?: (key: string) => void;
	nativeAdapter?: string | null;
} = $props();

type TimelineItem = { event: Event; pairedResult?: Event; resultOk?: boolean };
type EventGroup = 'user' | 'agent' | 'tool' | 'system';

const timelineEvents = $derived.by(() => events.filter((event) => !isBoilerplateEvent(event)));
const unifiedOptions = $derived.by(() => buildUnifiedFilterOptions(timelineEvents));
const nativeOptions = $derived.by(() => buildNativeFilterOptions(timelineEvents));
const activeOptions = $derived.by(() => (viewMode === 'native' ? nativeOptions : unifiedOptions));

function isFilterEnabled(optionKey: string): boolean {
	return viewMode === 'native'
		? nativeFilters.has(optionKey)
		: unifiedFilters.has(optionKey);
}

function toggleFilter(optionKey: string) {
	if (viewMode === 'native') {
		onToggleNativeFilter(optionKey);
		return;
	}
	onToggleUnifiedFilter(optionKey);
}

const filteredTimelineEvents = $derived.by(() => {
	if (viewMode === 'native') {
		return filterEventsByNativeGroups(timelineEvents, nativeFilters);
	}
	return filterEventsByUnifiedKeys(timelineEvents, unifiedFilters);
});

const timeline = $derived.by((): TimelineItem[] => {
	const pairs = pairToolCallResults(filteredTimelineEvents);
	return filteredTimelineEvents.map((event, idx) => {
		const paired = pairs.get(idx);
		return {
			event,
			pairedResult: paired,
			resultOk: paired ? !isToolError(paired.event_type) : false,
		};
	});
});

function getRoleGroup(event: Event): 'user' | 'agent' {
	if (event.event_type.type === 'UserMessage') return 'user';
	return 'agent';
}

function eventGroup(event: Event): EventGroup {
	const type = event.event_type.type;
	if (type === 'UserMessage') return 'user';
	if (type === 'SystemMessage') return 'system';
	if (type === 'AgentMessage' || type === 'Thinking' || type === 'TaskStart' || type === 'TaskEnd') {
		return 'agent';
	}
	return 'tool';
}

function timelineDotClass(event: Event): string {
	switch (eventGroup(event)) {
		case 'user':
			return 'bg-emerald-400';
		case 'agent':
			return 'bg-sky-400';
		case 'tool':
			return 'bg-amber-400';
		case 'system':
			return 'bg-slate-400';
	}
}

function handleFilterKeydown(e: KeyboardEvent) {
	if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
	const num = parseInt(e.key, 10);
	if (num < 1 || num > activeOptions.length) return;
	e.preventDefault();
	toggleFilter(activeOptions[num - 1].key);
}

function buttonLabel(option: FilterOption, index: number): string {
	return `${index + 1}: ${option.label} (${option.count})`;
}

const groupedCounts = $derived.by(() => {
	const counts: Record<EventGroup, number> = { user: 0, agent: 0, tool: 0, system: 0 };
	for (const event of filteredTimelineEvents) {
		counts[eventGroup(event)] += 1;
	}
	return counts;
});
</script>

<svelte:window onkeydown={handleFilterKeydown} />

{#snippet roleSeparator(currentRole: string)}
	<div class="my-1 pl-7 sm:my-5">
		<div class="flex items-center gap-3">
			<div class="h-px flex-1 bg-border/50"></div>
			<span class="text-[10px] font-medium uppercase tracking-wider text-text-muted">{currentRole}</span>
			<div class="h-px flex-1 bg-border/50"></div>
		</div>
	</div>
{/snippet}

<ThreadMinimap events={filteredTimelineEvents} />

<div>
	<div class="mb-4 rounded border border-border/80 bg-bg-secondary/55 p-2.5">
		<div class="flex flex-wrap items-center justify-between gap-2">
			<div class="flex items-center gap-1" role="tablist" aria-label="Session view mode">
				{#each [
					{ mode: 'unified', label: 'Unified', enabled: true },
					{
						mode: 'native',
						label: nativeAdapter ? `Native (${nativeAdapter})` : 'Native',
						enabled: nativeEnabled
					}
				] as btn}
					<button
						role="tab"
						aria-selected={viewMode === btn.mode}
						disabled={!btn.enabled}
						onclick={() => btn.enabled && onViewModeChange(btn.mode as SessionViewMode)}
						class="rounded border px-2 py-1 text-xs font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50
							{viewMode === btn.mode
							? 'border-accent/40 bg-accent/10 text-accent'
							: 'border-border bg-bg-primary text-text-secondary hover:text-text-primary'}"
					>
						{btn.label}
					</button>
				{/each}
			</div>

			<div class="flex flex-wrap items-center gap-1 text-[10px] text-text-muted">
				<span class="rounded border border-border bg-bg-primary px-1.5 py-0.5 text-text-secondary">
					{timeline.length} items
				</span>
				<span class="inline-flex items-center gap-1 rounded border border-border bg-bg-primary px-1.5 py-0.5">
					<span class="h-2 w-2 rounded-full bg-emerald-400"></span>
					{groupedCounts.user}
				</span>
				<span class="inline-flex items-center gap-1 rounded border border-border bg-bg-primary px-1.5 py-0.5">
					<span class="h-2 w-2 rounded-full bg-sky-400"></span>
					{groupedCounts.agent}
				</span>
				<span class="inline-flex items-center gap-1 rounded border border-border bg-bg-primary px-1.5 py-0.5">
					<span class="h-2 w-2 rounded-full bg-amber-400"></span>
					{groupedCounts.tool}
				</span>
				{#if groupedCounts.system > 0}
					<span class="inline-flex items-center gap-1 rounded border border-border bg-bg-primary px-1.5 py-0.5">
						<span class="h-2 w-2 rounded-full bg-slate-400"></span>
						{groupedCounts.system}
					</span>
				{/if}
			</div>
		</div>

		<div class="mt-2 flex flex-wrap items-center gap-1" role="group" aria-label="Event filters">
			{#each activeOptions as option, i}
				<button
					aria-pressed={isFilterEnabled(option.key)}
					onclick={() => toggleFilter(option.key)}
					class="rounded border px-2 py-0.5 text-[11px] font-medium transition-colors
						{isFilterEnabled(option.key)
						? 'border-accent/40 bg-accent/10 text-accent'
						: 'border-border bg-bg-primary text-text-muted hover:text-text-secondary'}"
				>
					{buttonLabel(option, i)}
				</button>
			{/each}
		</div>
	</div>

	{#if timeline.length === 0}
		<div class="border border-border bg-bg-secondary px-3 py-2 text-xs text-text-muted">
			No events match the selected filters.
		</div>
	{:else}
		<div class="overflow-x-auto pb-2">
			<div class="relative">
				<div class="pointer-events-none absolute bottom-0 left-[0.62rem] top-0 w-px bg-border/70"></div>
				{#each timeline as item, idx}
					{@const currentRole = getRoleGroup(item.event)}
					{@const previousRole = idx > 0 ? getRoleGroup(timeline[idx - 1].event) : null}
					{#if previousRole && previousRole !== currentRole}
						{@render roleSeparator(currentRole)}
					{/if}
					<div data-timeline-idx={idx} class="relative pl-7 transition-all">
						<span
							class={`pointer-events-none absolute left-[0.37rem] top-3 h-2.5 w-2.5 rounded-full ring-2 ring-bg-primary ${timelineDotClass(item.event)}`}
						></span>
						<div class="rounded border border-border/60 bg-bg-secondary/35 px-1.5 py-1 transition-colors hover:border-border-light/70 hover:bg-bg-secondary/65">
							<EventView event={item.event} pairedResult={item.pairedResult} resultOk={item.resultOk} />
						</div>
					</div>
				{/each}
			</div>
		</div>
	{/if}
</div>
