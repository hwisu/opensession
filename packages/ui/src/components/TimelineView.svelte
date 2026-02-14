<script lang="ts">
	import {
		applyTaskViewMode,
		collapseConsecutiveEvents,
		computeLaneEvents,
		computeMaxLane,
		computeTaskInfoMap,
		consecutiveGroupDisplayName,
		consecutiveGroupKey,
		consecutiveGroupSummary,
		elideRedundantFileReads,
		formatMs,
		getLaneColor,
		pairToolCallResults,
		taskBreakdown,
	} from '../timeline-helpers';
	import type { DisplayItem, LaneEvent, PairedToolCallItem } from '../timeline-types';
	import type { Event } from '../types';
	import EventView from './EventView.svelte';
	import { collapseIcon, expandIcon, stopIcon } from './icons';
	import ThreadMinimap from './ThreadMinimap.svelte';

	const { events }: { events: Event[] } = $props();

	type TimelineItem = { kind: 'standalone'; event: Event };

	let viewMode = $state<'timeline' | 'messages'>('timeline');
	const laneColumnGapPx = 260;
	let timelineScrollEl: HTMLDivElement | undefined = $state();
	let isDragging = $state(false);
	let dragStartX = $state(0);
	let dragStartScrollLeft = $state(0);

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
		if (['UserMessage', 'AgentMessage', 'SystemMessage'].includes(eventTypeName))
			return filters.messages;
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

	// --- Collapsed tasks ---
	let collapsedTasks = $state<Set<string>>(new Set());

	function toggleTask(taskId: string) {
		const next = new Set(collapsedTasks);
		if (next.has(taskId)) next.delete(taskId);
		else next.add(taskId);
		collapsedTasks = next;
	}

	// --- Task view mode (2 modes: summary-start / chronological) ---
	let taskViewMode = $state<'chronological' | 'summary-start'>('summary-start');

	function toggleTaskViewMode() {
		taskViewMode = taskViewMode === 'summary-start' ? 'chronological' : 'summary-start';
	}

	// --- Toggle states ---
	let collapseConsecutive = $state(true);

	// --- Check if session has sub-agents ---
	const hasSubAgents = $derived(events.some((e) => e.task_id != null));

	// --- Derived computations (pure functions) ---
	const laneEvents = $derived.by(() => computeLaneEvents(events));
	const taskInfoMap = $derived.by(() => computeTaskInfoMap(laneEvents));
	const maxLane = $derived.by(() => computeMaxLane(laneEvents));
	const minTrackWidth = $derived(`${Math.max(1, maxLane + 1) * laneColumnGapPx + 260}px`);

	// --- Display pipeline: applyTaskViewMode → elideRedundantFileReads → pairToolCallResults → collapseConsecutiveEvents ---
	const displayLaneEvents = $derived.by((): DisplayItem[] => {
		const afterTaskView = applyTaskViewMode(
			laneEvents,
			taskViewMode,
			collapsedTasks,
			taskInfoMap,
			matchesFilter,
		);
		const afterElide = elideRedundantFileReads(afterTaskView);
		const afterPair = pairToolCallResults(afterElide);
		return collapseConsecutive
			? collapseConsecutiveEvents(afterPair, consecutiveGroupKey)
			: afterPair;
	});

	// --- Messages mode (flat, no graph) ---
	const timeline = $derived.by((): TimelineItem[] => {
		const items = events.map((e) => ({ kind: 'standalone' as const, event: e }));
		if (viewMode === 'messages') {
			return items.filter((item) =>
				['UserMessage', 'AgentMessage'].includes(item.event.event_type.type),
			);
		}
		return items.filter((item) => matchesFilter(item.event.event_type.type));
	});

	const userMessageCount = $derived(events.filter((e) => e.event_type.type === 'UserMessage').length);

	function formatTime(ts: string): string {
		return new Date(ts).toLocaleTimeString('en-US', {
			hour12: false,
			hour: '2-digit',
			minute: '2-digit',
			second: '2-digit',
		});
	}

	function startDrag(e: PointerEvent) {
		if (viewMode !== 'timeline') return;
		const target = e.target as HTMLElement;
		if (target.closest('button') || target.closest('input') || target.closest('textarea') || target.closest('a')) {
			return;
		}

		e.preventDefault();
		isDragging = true;
		dragStartX = e.clientX;
		dragStartScrollLeft = timelineScrollEl?.scrollLeft ?? 0;
		if (e.currentTarget instanceof HTMLElement) {
			e.currentTarget.setPointerCapture(e.pointerId);
		}
	}

	function onDragMove(e: PointerEvent) {
		if (!isDragging || !timelineScrollEl) return;
		e.preventDefault();
		const delta = e.clientX - dragStartX;
		timelineScrollEl.scrollLeft = dragStartScrollLeft - delta;
	}

	function endDrag() {
		isDragging = false;
	}

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

<svelte:window
	onkeydown={handleFilterKeydown}
	onpointermove={onDragMove}
	onpointerup={endDrag}
	onpointercancel={endDrag}
/>

{#snippet roleSeparator(currentRole: string)}
	<div class="my-1 sm:my-6 flex items-center gap-3">
		<div class="h-px flex-1 bg-border/50"></div>
		<span class="text-[10px] font-medium uppercase tracking-wider text-text-muted">{currentRole}</span>
		<div class="h-px flex-1 bg-border/50"></div>
	</div>
{/snippet}

{#snippet flatEventList(items: TimelineItem[])}
	{#each items as item, idx}
		{@const currentRole = getRoleGroup(item.event)}
		{@const previousRole = idx > 0 ? getRoleGroup(items[idx - 1].event) : null}
		{#if previousRole && previousRole !== currentRole}
			{@render roleSeparator(currentRole)}
		{/if}
		<div data-timeline-idx={idx} class="transition-all">
			<EventView event={item.event} />
		</div>
	{/each}
{/snippet}

<!-- Minimap (desktop only) -->
<ThreadMinimap {events} />

<div>
	<!-- Top controls -->
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

			<div class="flex items-center gap-1.5">
				{#if hasSubAgents}
					<button
						onclick={toggleTaskViewMode}
						class="px-2 py-0.5 text-[11px] font-medium transition-colors
							{taskViewMode === 'chronological'
							? 'text-accent'
							: 'text-text-muted hover:text-text-secondary'}"
						title="Task view mode: {taskViewMode}"
					>
						Tasks:{taskViewMode === 'summary-start' ? 'Summary' : 'Detail'}
					</button>
				{/if}
				<button
					onclick={() => (collapseConsecutive = !collapseConsecutive)}
					class="px-2 py-0.5 text-[11px] font-medium transition-colors
						{collapseConsecutive
						? 'text-accent'
						: 'text-text-muted hover:text-text-secondary'}"
					title="Group consecutive repeated actions"
				>
					Group:{collapseConsecutive ? 'On' : 'Off'}
				</button>
			</div>
		{/if}

		<span class="text-xs text-text-muted">
			{viewMode === 'messages' ? timeline.length : displayLaneEvents.length} items
			{#if userMessageCount > 0}
				&middot; {userMessageCount} user messages
			{/if}
		</span>
	</div>

	<!-- Timeline content -->
	{#if viewMode === 'messages' || !hasSubAgents}
		<div
			bind:this={timelineScrollEl}
			class="overflow-x-auto pb-2"
			onpointerdown={startDrag}
			style="cursor: {isDragging ? 'grabbing' : 'grab'};"
			role="button"
			tabindex="0"
			aria-label="Drag horizontally to pan timeline"
		>
			<div>{@render flatEventList(timeline)}</div>
		</div>
	{:else}
		<!-- Timeline mode with sub-agents: lane columns -->
		<div
			bind:this={timelineScrollEl}
			class="overflow-x-auto pb-2"
			onpointerdown={startDrag}
			style="cursor: {isDragging ? 'grabbing' : 'grab'}; min-width: {minTrackWidth};"
			role="button"
			tabindex="0"
			aria-label="Drag horizontally to pan timeline"
		>
			<div class="pb-1" style="min-width: {minTrackWidth}">
				{#each displayLaneEvents as item, idx}
					{#if 'kind' in item && item.kind === 'collapsed'}
						{@const info = item.info}
						{@const breakdown = taskBreakdown(laneEvents, item.taskId)}
						{@const depth = item.lane}
						<div style="padding-left: {depth * laneColumnGapPx}px" data-timeline-idx={idx}>
							<button
								onclick={() => toggleTask(item.taskId)}
								class="group flex w-full items-center gap-2 border border-accent/20 bg-accent/5 px-3 py-1.5 text-left text-xs transition-colors hover:bg-bg-hover"
								style="border-left: 4px solid {getLaneColor(depth)}"
							>
								<span class="text-text-muted">{@html expandIcon}</span>
								<div class="flex-1 min-w-0">
									<span class="block font-semibold text-text-primary">
										{info.purpose || info.title || 'Sub-agent'}
									</span>
									<span class="block font-mono text-[10px] text-text-muted">{formatTime(info.startedAt)}{#if info.endedAt} → {formatTime(info.endedAt)}{/if}</span>
									{#if breakdown}
										<span class="block font-mono text-[10px] text-text-muted truncate">{breakdown}</span>
									{/if}
								</div>
								<span class="shrink-0 font-mono text-[10px] text-text-muted">{info.eventCount} events · {formatMs(info.durationMs)}</span>
							</button>
						</div>
					{:else if 'kind' in item && item.kind === 'paired'}
						{@const paired = item as PairedToolCallItem}
						{@const depth = paired.lane}
						{@const isNonMainLane = depth > 0}
						{@const isLowPriority = isNonMainLane && !['UserMessage', 'AgentMessage', 'TaskStart', 'TaskEnd'].includes(paired.callEvent.event.event_type.type)}
						<div style="padding-left: {depth * laneColumnGapPx}px" class:opacity-60={isLowPriority}>
							{#if depth > 0}
								<div style="border-left: 4px solid {getLaneColor(depth)}; padding-left: 0px">
									<EventView event={paired.callEvent.event} pairedResult={paired.resultEvent.event} />
								</div>
							{:else}
								<EventView event={paired.callEvent.event} pairedResult={paired.resultEvent.event} />
							{/if}
						</div>
					{:else if 'kind' in item && item.kind === 'consecutive'}
						{@const group = item}
						{@const depth = group.lane}
						<div style="padding-left: {depth * laneColumnGapPx}px" data-timeline-idx={idx}>
							{#if depth > 0}
								<div style="border-left: 4px solid {getLaneColor(depth)}; padding-left: 0px">
									<div class="flex items-center gap-2 border border-border/30 bg-bg-secondary/50 px-3 py-1.5 text-xs">
										<span class="shrink-0 rounded bg-text-muted/10 px-1.5 py-0.5 font-mono text-[10px] font-semibold text-text-muted">{group.count}&times;</span>
										<span class="shrink-0 font-medium text-text-secondary">{consecutiveGroupDisplayName(group.groupKey)}</span>
										<span class="min-w-0 flex-1 truncate font-mono text-[10px] text-text-muted">{consecutiveGroupSummary(group.events)}</span>
									</div>
								</div>
							{:else}
								<div class="flex items-center gap-2 border border-border/30 bg-bg-secondary/50 px-3 py-1.5 text-xs">
									<span class="shrink-0 rounded bg-text-muted/10 px-1.5 py-0.5 font-mono text-[10px] font-semibold text-text-muted">{group.count}&times;</span>
									<span class="shrink-0 font-medium text-text-secondary">{consecutiveGroupDisplayName(group.groupKey)}</span>
									<span class="min-w-0 flex-1 truncate font-mono text-[10px] text-text-muted">{consecutiveGroupSummary(group.events)}</span>
								</div>
							{/if}
						</div>
					{:else}
						{@const laneEvent = item as LaneEvent}
						{@const currentRole = getRoleGroup(laneEvent.event)}
						{@const prevItem = idx > 0 ? displayLaneEvents[idx - 1] : null}
						{@const previousRole = prevItem && !('kind' in prevItem) ? getRoleGroup((prevItem as LaneEvent).event) : null}
						{@const depth = laneEvent.lane}
						{@const isNonMainLane = depth > 0}
						{@const isLowPriorityInLane = isNonMainLane && !['UserMessage', 'AgentMessage', 'TaskStart', 'TaskEnd'].includes(laneEvent.event.event_type.type)}
						{#if depth === 0 && previousRole && previousRole !== currentRole}
							{@render roleSeparator(currentRole)}
						{/if}
						<div style="padding-left: {depth * laneColumnGapPx}px" class:opacity-60={isLowPriorityInLane} data-timeline-idx={idx}>
							{#if laneEvent.event.event_type.type === 'TaskStart' && laneEvent.event.task_id}
								{@const taskInfo = taskInfoMap.get(laneEvent.event.task_id)}
								<button
									onclick={() => toggleTask(laneEvent.event.task_id!)}
									class="group flex w-full items-center gap-2 border border-accent/20 bg-accent/5 px-3 py-1.5 text-left text-xs transition-colors hover:bg-bg-hover"
									style="border-left: 4px solid {getLaneColor(depth)}"
								>
									<span class="text-text-muted">{@html collapseIcon}</span>
									<span class="shrink-0 text-[10px] font-mono text-text-muted">{formatTime(laneEvent.event.timestamp)}</span>
									<span class="flex-1 font-semibold text-text-primary truncate">
										{taskInfo?.purpose || ('data' in laneEvent.event.event_type ? (laneEvent.event.event_type.data as { title?: string }).title || 'Sub-agent' : 'Sub-agent')}
									</span>
									{#if taskInfo}
										<span class="shrink-0 font-mono text-[10px] text-text-muted">{taskInfo.eventCount} events · {formatMs(taskInfo.durationMs)}{#if taskInfo.startedAt} · started {formatTime(taskInfo.startedAt)}{/if}{#if taskInfo.endedAt} · ended {formatTime(taskInfo.endedAt)}{/if}</span>
									{:else if laneEvent.event.duration_ms}
										<span class="shrink-0 font-mono text-[10px] text-text-muted">{laneEvent.event.duration_ms}ms</span>
									{/if}
								</button>
							{:else if laneEvent.event.event_type.type === 'TaskEnd' && laneEvent.event.task_id}
								{@const endTaskInfo = taskInfoMap.get(laneEvent.event.task_id)}
								<div
									class="flex items-center gap-2 border border-border/40 bg-bg-secondary/30 px-3 py-1 text-xs text-text-muted"
									style="border-left: 4px solid {getLaneColor(depth)}80"
								>
									<span class="inline-flex">{@html stopIcon}</span>
									<div class="shrink-0 text-[10px] font-mono text-text-muted">{formatTime(laneEvent.event.timestamp)}</div>
									<span class="font-mono text-[10px]">
										{'data' in laneEvent.event.event_type ? (laneEvent.event.event_type.data as { summary?: string }).summary || 'end' : 'end'}
									</span>
									{#if endTaskInfo}
										<span class="ml-auto shrink-0 font-mono text-[10px]">{formatMs(endTaskInfo.durationMs)}</span>
									{/if}
								</div>
							{:else if depth > 0}
								<div style="border-left: 4px solid {getLaneColor(depth)}; padding-left: 0px">
									<EventView event={laneEvent.event} />
								</div>
							{:else}
								<EventView event={laneEvent.event} />
							{/if}
					</div>
					{/if}
				{/each}
			</div>
		</div>
	{/if}
</div>
