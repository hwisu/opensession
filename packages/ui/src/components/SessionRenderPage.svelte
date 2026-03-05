<script lang="ts">
import { ApiError, buildSessionHandoff, quickShareSession } from '../api';
import { SCROLL_STEP_PX } from '../constants';
import { prepareTimelineEvents } from '../event-helpers';
import { isNativeAdapterSupported, type SessionViewMode } from '../session-filters';
import type { ContentBlock, Event, Session, SessionDetail } from '../types';
import { formatDuration, formatTimestamp, getToolConfig } from '../types';
import { computeFileStats, getDisplayTitle } from '../utils';
import SessionSidebar from './SessionSidebar.svelte';
import TimelineView from './TimelineView.svelte';

const {
	session,
	detail = null,
	viewMode = 'unified',
	nativeAdapter = null,
	unifiedFilters = new Set<string>(),
	branchFilters = new Set<string>(),
	nativeFilters = new Set<string>(),
	onViewModeChange = (_mode: SessionViewMode) => {},
	onToggleUnifiedFilter = (_key: string) => {},
	onToggleBranchFilter = (_key: string) => {},
	onToggleNativeFilter = (_key: string) => {},
}: {
	session: Session;
	detail?: SessionDetail | null;
	viewMode?: SessionViewMode;
	nativeAdapter?: string | null;
	unifiedFilters?: Set<string>;
	branchFilters?: Set<string>;
	nativeFilters?: Set<string>;
	onViewModeChange?: (mode: SessionViewMode) => void;
	onToggleUnifiedFilter?: (key: string) => void;
	onToggleBranchFilter?: (key: string) => void;
	onToggleNativeFilter?: (key: string) => void;
} = $props();

let searchQuery = $state('');
let searchInput: HTMLInputElement | undefined = $state();
let searchCursor = $state(-1);
let timelineEl: HTMLDivElement | undefined = $state();
let flowTrackEl: HTMLButtonElement | undefined = $state();
let flowDragging = $state(false);
let flowDragPointerId: number | null = null;
let flowDragRaf: number | null = null;
let flowPendingRatio: number | null = null;
let flowTargetItems: HTMLElement[] = [];
let flowLastTargetIdx: number | null = null;
let handoffPending = $state(false);
let handoffArtifactUri = $state<string | null>(null);
let handoffDownloadFileName = $state<string | null>(null);
let handoffDownloadContent = $state<string | null>(null);
let handoffFeedback = $state<string | null>(null);
let handoffFeedbackLevel = $state<'success' | 'error' | null>(null);
let quickSharePending = $state(false);
let quickShareRemote = $state('');
let quickShareUri = $state<string | null>(null);
let quickShareFeedback = $state<string | null>(null);
let quickShareFeedbackLevel = $state<'success' | 'error' | null>(null);

type FlowKind = 'user' | 'agent' | 'tool' | 'system';
type FlowSegment = { kind: FlowKind; width: number; tooltip: string };

function eventFlowKind(event: Event): FlowKind {
	const type = event.event_type.type;
	if (type === 'UserMessage') return 'user';
	if (type === 'SystemMessage') return 'system';
	if (
		type === 'AgentMessage' ||
		type === 'Thinking' ||
		type === 'TaskStart' ||
		type === 'TaskEnd'
	) {
		return 'agent';
	}
	return 'tool';
}

function flowBarClass(kind: FlowKind): string {
	switch (kind) {
		case 'user':
			return 'bg-emerald-400/80';
		case 'agent':
			return 'bg-sky-400/80';
		case 'tool':
			return 'bg-amber-400/80';
		case 'system':
			return 'bg-slate-400/80';
	}
}

function flowDotClass(kind: FlowKind): string {
	switch (kind) {
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

const tool = $derived(getToolConfig(session.agent.tool));
const displayTitle = $derived(getDisplayTitle(session));
const fileStats = $derived(computeFileStats(session.events));
const normalizedSearchQuery = $derived(searchQuery.trim().toLowerCase());
const timelineEvents = $derived.by(() => prepareTimelineEvents(session.events));
const flowEvents = $derived(session.events);
const nativeEnabled = $derived(isNativeAdapterSupported(nativeAdapter));
const desktopRuntime = $derived.by(() => {
	if (typeof window === 'undefined') return false;
	const maybeTauri = window as Window & { __TAURI_INTERNALS__?: unknown };
	return '__TAURI_INTERNALS__' in maybeTauri || window.location.protocol === 'tauri:';
});
const effectiveViewMode = $derived(viewMode === 'native' && !nativeEnabled ? 'unified' : viewMode);

const searchableEvents = $derived.by(() => {
	return timelineEvents.map((event) => ({
		event,
		searchText: eventToSearchText(event),
	}));
});

const searchFilteredEvents = $derived.by(() => {
	if (!normalizedSearchQuery) return timelineEvents;
	return searchableEvents
		.filter((entry) => entry.searchText.includes(normalizedSearchQuery))
		.map((entry) => entry.event);
});

const searchMatchCount = $derived(normalizedSearchQuery ? searchFilteredEvents.length : 0);

const flowCounts = $derived.by(() => {
	const counts: Record<FlowKind, number> = {
		user: 0,
		agent: 0,
		tool: 0,
		system: 0,
	};
	for (const event of flowEvents) {
		counts[eventFlowKind(event)] += 1;
	}
	return counts;
});

const flowSegments = $derived.by((): FlowSegment[] => {
	if (flowEvents.length === 0) return [];
	const width = 100 / flowEvents.length;
	return flowEvents.map((event, index) => {
		const kind = eventFlowKind(event);
		return {
			kind,
			width,
			tooltip: `${index + 1}. ${event.event_type.type}`,
		};
	});
});

const flowLegend = $derived.by(() => {
	return [
		{ kind: 'user' as const, label: 'User', count: flowCounts.user },
		{ kind: 'agent' as const, label: 'Agent', count: flowCounts.agent },
		{ kind: 'tool' as const, label: 'Tool', count: flowCounts.tool },
		{ kind: 'system' as const, label: 'System', count: flowCounts.system },
	];
});

$effect(() => {
	if (viewMode === 'native' && !nativeEnabled) {
		onViewModeChange('unified');
	}
});

$effect(() => {
	void normalizedSearchQuery;
	searchCursor = -1;
});

$effect(() => {
	if (typeof window === 'undefined') return;
	const handler = () => {
		focusSearchInput();
	};
	window.addEventListener('opensession:focus-search', handler);
	return () => {
		window.removeEventListener('opensession:focus-search', handler);
	};
});

function isSearchFocusShortcut(e: KeyboardEvent): boolean {
	if (e.key.toLowerCase() === 'f' && (e.metaKey || e.ctrlKey)) return true;
	return e.code === 'Slash' || e.key === '/';
}

function handleKeydown(e: KeyboardEvent) {
	if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
	if (isSearchFocusShortcut(e)) {
		e.preventDefault();
		focusSearchInput();
		return;
	}
	if (normalizedSearchQuery && e.key.toLowerCase() === 'n') {
		e.preventDefault();
		focusSearchMatch(1);
		return;
	}
	if (normalizedSearchQuery && e.key.toLowerCase() === 'p') {
		e.preventDefault();
		focusSearchMatch(-1);
		return;
	}
	if (!timelineEl) return;
	if (e.key === 'j') {
		e.preventDefault();
		timelineEl.scrollBy({ top: SCROLL_STEP_PX, behavior: 'smooth' });
	} else if (e.key === 'k') {
		e.preventDefault();
		timelineEl.scrollBy({ top: -SCROLL_STEP_PX, behavior: 'smooth' });
	}
}

function normalizeForSearch(value: unknown): string {
	if (value == null) return '';
	if (typeof value === 'string') return value.toLowerCase();
	if (typeof value === 'number' || typeof value === 'boolean') return String(value).toLowerCase();
	try {
		return JSON.stringify(value).toLowerCase();
	} catch {
		return '';
	}
}

function blockToSearchText(block: ContentBlock): string {
	switch (block.type) {
		case 'Text':
			return normalizeForSearch(block.text);
		case 'Code':
			return `${normalizeForSearch(block.language)}\n${normalizeForSearch(block.code)}`;
		case 'Json':
			return normalizeForSearch(block.data);
		case 'File':
			return `${normalizeForSearch(block.path)}\n${normalizeForSearch(block.content)}`;
		case 'Image':
		case 'Audio':
		case 'Video':
			return `${normalizeForSearch(block.url)}\n${normalizeForSearch('alt' in block ? block.alt : '')}`;
		case 'Reference':
			return `${normalizeForSearch(block.uri)}\n${normalizeForSearch(block.media_type)}`;
		default:
			return '';
	}
}

function eventToSearchText(event: Event): string {
	const typeData = 'data' in event.event_type ? normalizeForSearch(event.event_type.data) : '';
	const contentText = event.content.blocks.map((block) => blockToSearchText(block)).join('\n');
	return [
		event.event_type.type,
		typeData,
		contentText,
		normalizeForSearch(event.attributes),
		normalizeForSearch(event.task_id),
	].join('\n');
}

function focusSearchInput() {
	searchInput?.focus();
	searchInput?.select();
}

function focusSearchMatch(direction: 1 | -1 = 1) {
	if (!timelineEl) return;
	const items = Array.from(timelineEl.querySelectorAll<HTMLElement>('[data-timeline-idx]'));
	if (items.length === 0) return;
	if (searchCursor < 0 || searchCursor >= items.length) {
		searchCursor = direction === 1 ? 0 : items.length - 1;
	} else {
		searchCursor = (searchCursor + direction + items.length) % items.length;
	}
	const target = items[searchCursor];
	target.scrollIntoView({ behavior: 'smooth', block: 'center' });
}

function handleSearchInputKeydown(e: KeyboardEvent) {
	if (e.key === 'Enter') {
		e.preventDefault();
		focusSearchMatch(e.shiftKey ? -1 : 1);
		return;
	}
	if (e.key === 'Escape') {
		e.preventDefault();
		if (searchQuery.trim().length > 0) {
			searchQuery = '';
			searchCursor = -1;
			return;
		}
		searchInput?.blur();
	}
}

function setHandoffFeedback(message: string | null, level: 'success' | 'error' | null = null) {
	handoffFeedback = message;
	handoffFeedbackLevel = level;
}

function setQuickShareFeedback(message: string | null, level: 'success' | 'error' | null = null) {
	quickShareFeedback = message;
	quickShareFeedbackLevel = level;
}

async function writeClipboardText(text: string): Promise<boolean> {
	if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
		try {
			await navigator.clipboard.writeText(text);
			return true;
		} catch {
			// Fall through to legacy copy path.
		}
	}
	if (typeof document === 'undefined') return false;
	const textarea = document.createElement('textarea');
	textarea.value = text;
	textarea.setAttribute('readonly', '');
	textarea.style.position = 'fixed';
	textarea.style.opacity = '0';
	document.body.appendChild(textarea);
	textarea.select();
	let copied = false;
	try {
		copied = document.execCommand('copy');
	} catch {
		copied = false;
	}
	textarea.remove();
	return copied;
}

async function handleBuildHandoff() {
	if (handoffPending) return;
	handoffPending = true;
	setHandoffFeedback(null);
	try {
		const response = await buildSessionHandoff(session.session_id, true);
		handoffArtifactUri = response.artifact_uri;
		const downloadFileName = response.download_file_name ?? null;
		const downloadContent = response.download_content ?? null;
		handoffDownloadFileName = downloadFileName;
		handoffDownloadContent = downloadContent;
		let downloaded = false;
		if (downloadFileName && downloadContent) {
			downloaded = downloadTextFile(downloadFileName, downloadContent);
		}
		setHandoffFeedback(
			response.pinned_alias
				? downloaded
					? `Handoff artifact built, pinned as ${response.pinned_alias}, and downloaded.`
					: `Handoff artifact built and pinned as ${response.pinned_alias}.`
				: downloaded
					? 'Handoff artifact built and downloaded.'
					: 'Handoff artifact built.',
			'success',
		);
	} catch (error) {
		const message = error instanceof ApiError ? error.message : 'Failed to build handoff artifact.';
		setHandoffFeedback(message, 'error');
	} finally {
		handoffPending = false;
	}
}

async function handleCopyHandoffUri() {
	if (!handoffArtifactUri) return;
	const copied = await writeClipboardText(handoffArtifactUri);
	setHandoffFeedback(
		copied ? 'Artifact URI copied.' : 'Failed to copy artifact URI.',
		copied ? 'success' : 'error',
	);
}

async function handleQuickShare() {
	if (quickSharePending) return;
	quickSharePending = true;
	setQuickShareFeedback(null);
	try {
		const response = await quickShareSession(session.session_id, quickShareRemote.trim() || null);
		quickShareUri = response.shared_uri;
		setQuickShareFeedback(
			response.pushed
				? 'Shared and pushed successfully.'
				: 'Shared locally. Confirm first push once to enable auto push in quick mode.',
			'success',
		);
	} catch (error) {
		const message = error instanceof ApiError ? error.message : 'Failed to quick share session.';
		setQuickShareFeedback(message, 'error');
	} finally {
		quickSharePending = false;
	}
}

async function handleCopyQuickShareUri() {
	if (!quickShareUri) return;
	const copied = await writeClipboardText(quickShareUri);
	setQuickShareFeedback(
		copied ? 'Shared URI copied.' : 'Failed to copy shared URI.',
		copied ? 'success' : 'error',
	);
}

function downloadTextFile(fileName: string, content: string): boolean {
	if (typeof document === 'undefined') return false;
	try {
		const blob = new Blob([content], { type: 'application/x-ndjson;charset=utf-8' });
		const url = URL.createObjectURL(blob);
		const link = document.createElement('a');
		link.href = url;
		link.download = fileName;
		link.style.display = 'none';
		document.body.appendChild(link);
		link.click();
		link.remove();
		URL.revokeObjectURL(url);
		return true;
	} catch {
		return false;
	}
}

function handleDownloadHandoffFile() {
	if (!handoffDownloadFileName || !handoffDownloadContent) return;
	const downloaded = downloadTextFile(handoffDownloadFileName, handoffDownloadContent);
	setHandoffFeedback(
		downloaded
			? `Downloaded ${handoffDownloadFileName}.`
			: 'Failed to download handoff artifact file.',
		downloaded ? 'success' : 'error',
	);
}

function clamp01(value: number): number {
	if (value < 0) return 0;
	if (value > 1) return 1;
	return value;
}

function flowRatioFromClientX(clientX: number): number | null {
	const rect = flowTrackEl?.getBoundingClientRect();
	if (!rect || rect.width <= 0) return null;
	return clamp01((clientX - rect.left) / rect.width);
}

function applyFlowRatioScroll(ratio: number) {
	if (!timelineEl) return;
	if (flowTargetItems.length === 0) {
		flowTargetItems = Array.from(timelineEl.querySelectorAll<HTMLElement>('[data-timeline-idx]'));
	}
	if (flowTargetItems.length === 0) return;
	const nextIdx = Math.round(clamp01(ratio) * (flowTargetItems.length - 1));
	if (flowLastTargetIdx === nextIdx) return;
	flowLastTargetIdx = nextIdx;
	flowTargetItems[nextIdx]?.scrollIntoView({ behavior: 'auto', block: 'center' });
}

function flushFlowPendingScroll() {
	if (flowPendingRatio == null) return;
	applyFlowRatioScroll(flowPendingRatio);
	flowPendingRatio = null;
}

function scheduleFlowRatioScroll(ratio: number) {
	flowPendingRatio = ratio;
	if (flowDragRaf != null) return;
	flowDragRaf = requestAnimationFrame(() => {
		flowDragRaf = null;
		flushFlowPendingScroll();
	});
}

function finishFlowDrag(pointerId: number | null) {
	if (flowDragPointerId == null) return;
	if (pointerId != null && pointerId !== flowDragPointerId) return;
	if (flowTrackEl?.hasPointerCapture(flowDragPointerId)) {
		flowTrackEl.releasePointerCapture(flowDragPointerId);
	}
	flowDragPointerId = null;
	flowDragging = false;
	flushFlowPendingScroll();
	flowTargetItems = [];
	flowLastTargetIdx = null;
}

function handleFlowTrackPointerDown(event: PointerEvent) {
	if (event.button !== 0) return;
	const ratio = flowRatioFromClientX(event.clientX);
	if (ratio == null) return;
	event.preventDefault();
	flowTrackEl?.setPointerCapture(event.pointerId);
	flowDragPointerId = event.pointerId;
	flowDragging = true;
	flowTargetItems = Array.from(
		timelineEl?.querySelectorAll<HTMLElement>('[data-timeline-idx]') ?? [],
	);
	flowLastTargetIdx = null;
	applyFlowRatioScroll(ratio);
}

function handleFlowTrackPointerMove(event: PointerEvent) {
	if (!flowDragging || flowDragPointerId == null || event.pointerId !== flowDragPointerId) return;
	const ratio = flowRatioFromClientX(event.clientX);
	if (ratio == null) return;
	event.preventDefault();
	scheduleFlowRatioScroll(ratio);
}

function handleFlowTrackPointerUp(event: PointerEvent) {
	if (flowDragPointerId == null || event.pointerId !== flowDragPointerId) return;
	const ratio = flowRatioFromClientX(event.clientX);
	if (ratio != null) {
		scheduleFlowRatioScroll(ratio);
	}
	finishFlowDrag(event.pointerId);
}

function handleFlowTrackPointerCancel(event: PointerEvent) {
	finishFlowDrag(event.pointerId);
}

$effect(() => {
	return () => {
		if (flowDragRaf != null) {
			cancelAnimationFrame(flowDragRaf);
			flowDragRaf = null;
		}
		flowPendingRatio = null;
		flowDragPointerId = null;
		flowDragging = false;
		flowTargetItems = [];
		flowLastTargetIdx = null;
	};
});
</script>

<svelte:window onkeydown={handleKeydown} />

<svelte:head>
	<title>{displayTitle} - opensession.io</title>
</svelte:head>

<div class="flex h-full flex-col">
	<div
		data-testid="session-detail-hero"
		class="session-hero relative shrink-0 border-b border-border/70 px-3 py-3"
	>
		<div class="session-hero-glow pointer-events-none absolute inset-x-0 top-0 h-16"></div>
		<div class="relative">
			<h1 class="break-words whitespace-normal select-text text-base font-semibold text-text-primary sm:text-lg">
				{displayTitle}
			</h1>

			<div class="mt-2 flex flex-wrap items-center gap-1.5 text-[11px] text-text-muted">
				<span class="inline-flex items-center gap-1 rounded border border-border bg-bg-primary/65 px-2 py-0.5">
					<span class="tui-badge tui-badge-tool" style="background-color: {tool.color}">{tool.icon}</span>
					<span class="text-text-secondary">{tool.label}</span>
				</span>
				<span class="rounded border border-border bg-bg-primary/65 px-2 py-0.5 text-text-secondary">
					{session.agent.model}
				</span>
				<span class="rounded border border-border bg-bg-primary/65 px-2 py-0.5">
					{formatDuration(session.stats.duration_seconds)}
				</span>
				<span class="rounded border border-border bg-bg-primary/65 px-2 py-0.5">
					{session.stats.message_count} msgs
				</span>
				{#if fileStats.filesChanged > 0}
					<span class="rounded border border-border bg-bg-primary/65 px-2 py-0.5">
						{fileStats.filesChanged} files
						(<span class="text-success">+{fileStats.linesAdded}</span>
						<span class="text-error">-{fileStats.linesRemoved}</span>)
					</span>
				{/if}
				<span class="rounded border border-border bg-bg-primary/65 px-2 py-0.5">
					{formatTimestamp(session.context.created_at)}
				</span>
			</div>

			<div class="mt-3 rounded border border-border/80 bg-bg-secondary/55 p-2.5" data-testid="session-flow-bar">
				<div class="flex items-center justify-between text-[11px] text-text-muted">
					<span class="font-medium text-text-secondary">Session Flow</span>
					<span>{flowEvents.length} events</span>
				</div>
				{#if flowSegments.length > 0}
					<button
						type="button"
						aria-label="Drag to scrub session timeline"
						bind:this={flowTrackEl}
						data-testid="session-flow-track"
						class="mt-2 flex h-2 overflow-hidden rounded-sm border bg-bg-tertiary/80 select-none touch-none cursor-ew-resize {flowDragging ? 'border-accent/70' : 'border-border/70'}"
						onpointerdown={handleFlowTrackPointerDown}
						onpointermove={handleFlowTrackPointerMove}
						onpointerup={handleFlowTrackPointerUp}
						onpointercancel={handleFlowTrackPointerCancel}
					>
						{#each flowSegments as segment}
							<span
								class={`h-full ${flowBarClass(segment.kind)}`}
								style={`width:${segment.width}%`}
								title={segment.tooltip}
							></span>
						{/each}
					</button>
				{/if}
				<div class="mt-2 flex flex-wrap items-center gap-1.5 text-[10px] text-text-muted">
					{#each flowLegend as item}
						<span class="inline-flex items-center gap-1 rounded border border-border/70 bg-bg-primary/70 px-1.5 py-0.5">
							<span class={`h-2 w-2 rounded-full ${flowDotClass(item.kind)}`}></span>
							<span>{item.label}</span>
							<span class="text-text-secondary">{item.count}</span>
						</span>
					{/each}
				</div>
			</div>

			<div class="mt-3 flex flex-wrap items-center gap-2 rounded border border-border/80 bg-bg-secondary/55 p-2">
				<label
					for="session-event-search"
					class="inline-flex h-6 w-6 items-center justify-center rounded border border-border bg-bg-primary text-xs text-text-muted"
				>
					/
				</label>
				<input
					id="session-event-search"
					type="text"
					bind:this={searchInput}
					bind:value={searchQuery}
					onkeydown={handleSearchInputKeydown}
					placeholder="search in this session..."
					class="min-w-[220px] flex-1 border border-border bg-bg-primary px-2 py-1 text-xs text-text-primary placeholder-text-muted outline-none focus:border-accent"
				/>
				{#if normalizedSearchQuery}
					<span
						class="rounded border border-border bg-bg-primary px-2 py-1 text-xs"
						class:text-warning={searchMatchCount === 0}
						class:text-text-muted={searchMatchCount > 0}
					>
						{searchMatchCount} matches
					</span>
				{/if}
			</div>

			{#if desktopRuntime}
				<div
					data-testid="session-handoff-panel"
					class="mt-3 rounded border border-border/80 bg-bg-secondary/55 p-2"
				>
					<div class="mb-3 rounded border border-border/60 bg-bg-primary/35 p-2">
						<div class="flex flex-wrap items-center justify-between gap-2">
							<div class="text-xs text-text-secondary">Public Share</div>
							<div class="flex flex-wrap items-center gap-2">
								<input
									type="text"
									bind:value={quickShareRemote}
									placeholder="remote (optional, auto-detect by default)"
									class="w-[260px] border border-border bg-bg-primary px-2 py-1 text-xs text-text-primary"
								/>
								<button
									type="button"
									data-testid="session-quick-share-run"
									onclick={handleQuickShare}
									disabled={quickSharePending}
									class="rounded border border-border bg-bg-primary px-2 py-1 text-xs text-text-secondary transition-colors hover:text-text-primary disabled:opacity-60"
								>
									{quickSharePending ? 'Sharing...' : 'Quick Share'}
								</button>
								{#if quickShareUri}
									<button
										type="button"
										data-testid="session-quick-share-copy"
										onclick={handleCopyQuickShareUri}
										class="rounded border border-border bg-bg-primary px-2 py-1 text-xs text-text-secondary transition-colors hover:text-text-primary"
									>
										Copy URI
									</button>
								{/if}
							</div>
						</div>
						{#if quickShareUri}
							<div
								data-testid="session-quick-share-uri"
								class="mt-2 rounded border border-border/70 bg-bg-primary px-2 py-1 font-mono text-[11px] text-text-secondary"
							>
								{quickShareUri}
							</div>
						{/if}
						{#if quickShareFeedback}
							<div
								data-testid="session-quick-share-feedback"
								class="mt-2 text-xs"
								class:text-success={quickShareFeedbackLevel === 'success'}
								class:text-error={quickShareFeedbackLevel === 'error'}
								class:text-text-muted={quickShareFeedbackLevel == null}
							>
								{quickShareFeedback}
							</div>
						{/if}
					</div>

					<div class="flex flex-wrap items-center justify-between gap-2">
						<div class="text-xs text-text-secondary">Handoff Artifact</div>
						<div class="flex items-center gap-2">
							<button
								type="button"
								data-testid="session-handoff-build"
								onclick={handleBuildHandoff}
								disabled={handoffPending}
								class="rounded border border-border bg-bg-primary px-2 py-1 text-xs text-text-secondary transition-colors hover:text-text-primary disabled:opacity-60"
							>
								{handoffPending ? 'Building...' : 'Build (pin latest)'}
							</button>
							{#if handoffArtifactUri}
								<button
									type="button"
									data-testid="session-handoff-copy"
									onclick={handleCopyHandoffUri}
									class="rounded border border-border bg-bg-primary px-2 py-1 text-xs text-text-secondary transition-colors hover:text-text-primary"
								>
									Copy URI
								</button>
							{/if}
							{#if handoffDownloadFileName && handoffDownloadContent}
								<button
									type="button"
									data-testid="session-handoff-download"
									onclick={handleDownloadHandoffFile}
									class="rounded border border-border bg-bg-primary px-2 py-1 text-xs text-text-secondary transition-colors hover:text-text-primary"
								>
									Download file
								</button>
							{/if}
						</div>
					</div>
					{#if handoffArtifactUri}
						<div
							data-testid="session-handoff-uri"
							class="mt-2 rounded border border-border/70 bg-bg-primary px-2 py-1 font-mono text-[11px] text-text-secondary"
						>
							{handoffArtifactUri}
						</div>
					{/if}
					{#if handoffFeedback}
						<div
							data-testid="session-handoff-feedback"
							class="mt-2 text-xs"
							class:text-success={handoffFeedbackLevel === 'success'}
							class:text-error={handoffFeedbackLevel === 'error'}
							class:text-text-muted={handoffFeedbackLevel == null}
						>
							{handoffFeedback}
						</div>
					{/if}
				</div>
			{/if}
		</div>
	</div>

	{#if viewMode === 'native' && !nativeEnabled}
		<div class="border-b border-border bg-bg-secondary px-3 py-2 text-xs text-text-muted">
			Native view is unavailable for this parser. Falling back to unified view.
		</div>
	{/if}

	<div class="session-body flex min-h-0 flex-1 overflow-hidden">
		<div
			bind:this={timelineEl}
			data-testid="session-timeline-scroll"
			class="relative flex-1 overflow-y-auto px-3 py-3"
		>
			{#if normalizedSearchQuery && searchMatchCount === 0}
				<div class="mb-2 border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">
					No matching events for "{searchQuery}".
				</div>
			{/if}
			<TimelineView
				events={searchFilteredEvents}
				viewMode={effectiveViewMode}
				nativeEnabled={nativeEnabled}
				{nativeAdapter}
				{unifiedFilters}
				{branchFilters}
				{nativeFilters}
				{onViewModeChange}
				{onToggleUnifiedFilter}
				{onToggleBranchFilter}
				{onToggleNativeFilter}
			/>
		</div>
		<SessionSidebar {session} {detail} {fileStats} />
	</div>
</div>

<style>
	.session-hero {
		background: linear-gradient(
			180deg,
			color-mix(in oklab, var(--color-bg-secondary) 88%, transparent),
			color-mix(in oklab, var(--color-bg-primary) 92%, transparent)
		);
	}

	.session-hero-glow {
		background: radial-gradient(
			circle at top right,
			color-mix(in oklab, var(--color-accent) 24%, transparent),
			transparent 65%
		);
	}

	.session-body {
		background: radial-gradient(
			circle at top left,
			color-mix(in oklab, var(--color-accent) 8%, transparent),
			transparent 60%
		);
	}
</style>
