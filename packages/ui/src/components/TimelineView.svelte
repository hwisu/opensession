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

const userMessageCount = $derived(
	filteredTimelineEvents.filter((event) => event.event_type.type === 'UserMessage').length,
);

function getRoleGroup(event: Event): 'user' | 'agent' {
	if (event.event_type.type === 'UserMessage') return 'user';
	return 'agent';
}

function handleFilterKeydown(e: KeyboardEvent) {
	if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
	const num = parseInt(e.key, 10);
	if (num < 1 || num > activeOptions.length) return;
	e.preventDefault();
	toggleFilter(activeOptions[num - 1].key);
}

function buttonLabel(option: FilterOption, index: number): string {
	return `${index + 1}:${option.label} (${option.count})`;
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

<ThreadMinimap events={filteredTimelineEvents} />

<div>
	<div class="mb-4 flex flex-wrap items-center gap-2">
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
					class="px-2 py-0.5 text-xs font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50
						{viewMode === btn.mode
						? 'text-accent'
						: 'text-text-secondary hover:text-text-primary'}"
				>
					{viewMode === btn.mode ? `[${btn.label}]` : btn.label}
				</button>
			{/each}
		</div>

		<div class="flex flex-wrap items-center gap-1" role="group" aria-label="Event filters">
			{#each activeOptions as option, i}
				<button
					aria-pressed={isFilterEnabled(option.key)}
					onclick={() => toggleFilter(option.key)}
					class="px-2 py-0.5 text-[11px] font-medium transition-colors
						{isFilterEnabled(option.key)
						? 'text-accent'
						: 'text-text-muted hover:text-text-secondary'}"
				>
					{buttonLabel(option, i)}
				</button>
			{/each}
		</div>

		<span class="text-xs text-text-muted">
			{timeline.length} items
			{#if userMessageCount > 0}
				&middot; {userMessageCount} user messages
			{/if}
		</span>
	</div>

	{#if timeline.length === 0}
		<div class="border border-border bg-bg-secondary px-3 py-2 text-xs text-text-muted">
			No events match the selected filters.
		</div>
	{:else}
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
	{/if}
</div>
