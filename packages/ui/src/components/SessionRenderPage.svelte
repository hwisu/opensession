<script lang="ts">
import { SCROLL_STEP_PX } from '../constants';
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
	nativeFilters = new Set<string>(),
	onViewModeChange = (_mode: SessionViewMode) => {},
	onToggleUnifiedFilter = (_key: string) => {},
	onToggleNativeFilter = (_key: string) => {},
}: {
	session: Session;
	detail?: SessionDetail | null;
	viewMode?: SessionViewMode;
	nativeAdapter?: string | null;
	unifiedFilters?: Set<string>;
	nativeFilters?: Set<string>;
	onViewModeChange?: (mode: SessionViewMode) => void;
	onToggleUnifiedFilter?: (key: string) => void;
	onToggleNativeFilter?: (key: string) => void;
} = $props();

let searchQuery = $state('');
let searchInput: HTMLInputElement | undefined = $state();
let searchCursor = $state(-1);
let timelineEl: HTMLDivElement | undefined = $state();

const tool = $derived(getToolConfig(session.agent.tool));
const displayTitle = $derived(getDisplayTitle(session));
const fileStats = $derived(computeFileStats(session.events));
const normalizedSearchQuery = $derived(searchQuery.trim().toLowerCase());
const nativeEnabled = $derived(isNativeAdapterSupported(nativeAdapter));
const effectiveViewMode = $derived(
	viewMode === 'native' && !nativeEnabled ? 'unified' : viewMode,
);

const searchableEvents = $derived.by(() => {
	return session.events.map((event) => ({
		event,
		searchText: eventToSearchText(event),
	}));
});

const searchFilteredEvents = $derived.by(() => {
	if (!normalizedSearchQuery) return session.events;
	return searchableEvents
		.filter((entry) => entry.searchText.includes(normalizedSearchQuery))
		.map((entry) => entry.event);
});

const searchMatchCount = $derived(normalizedSearchQuery ? searchFilteredEvents.length : 0);

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
	return e.code === 'Slash' || e.key === '/' || e.key === '?';
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
</script>

<svelte:window onkeydown={handleKeydown} />

<svelte:head>
	<title>{displayTitle} - opensession.io</title>
</svelte:head>

<div class="flex h-full flex-col">
	<div class="shrink-0 border-b border-border px-3 py-2">
		<h1 class="truncate text-lg font-bold text-text-primary">
			{displayTitle}
		</h1>
		<div class="mt-1 flex flex-wrap items-center gap-2 text-xs text-text-muted">
			<span class="tui-badge tui-badge-tool" style="background-color: {tool.color}">{tool.icon}</span>
			<span>{tool.label}</span>
			<span>&middot;</span>
			<span class="text-text-secondary">{session.agent.model}</span>
			<span>&middot;</span>
			<span>{formatDuration(session.stats.duration_seconds)}</span>
			<span>&middot;</span>
			<span>{session.stats.message_count} msgs</span>
			{#if fileStats.filesChanged > 0}
				<span>&middot;</span>
				<span>{fileStats.filesChanged} files
					(<span class="text-success">+{fileStats.linesAdded}</span>
					<span class="text-error">-{fileStats.linesRemoved}</span>)
				</span>
			{/if}
			<span>&middot;</span>
			<span>{formatTimestamp(session.context.created_at)}</span>
		</div>
		<div class="mt-2 flex flex-wrap items-center gap-2">
			<label for="session-event-search" class="text-xs text-text-muted">/</label>
			<input
				id="session-event-search"
				type="text"
				bind:this={searchInput}
				bind:value={searchQuery}
				onkeydown={handleSearchInputKeydown}
				placeholder="search in this session..."
				class="min-w-[220px] flex-1 border border-border bg-bg-secondary px-2 py-1 text-xs text-text-primary placeholder-text-muted outline-none focus:border-accent"
			/>
			{#if normalizedSearchQuery}
				<span
					class="text-xs"
					class:text-warning={searchMatchCount === 0}
					class:text-text-muted={searchMatchCount > 0}
				>
					{searchMatchCount} matches
				</span>
			{/if}
		</div>
	</div>

	{#if viewMode === 'native' && !nativeEnabled}
		<div class="border-b border-border bg-bg-secondary px-3 py-2 text-xs text-text-muted">
			Native view is unavailable for this parser. Falling back to unified view.
		</div>
	{/if}

	<div class="flex min-h-0 flex-1 overflow-hidden">
		<div bind:this={timelineEl} class="flex-1 overflow-y-auto px-3 py-2">
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
				{nativeFilters}
				{onViewModeChange}
				{onToggleUnifiedFilter}
				{onToggleNativeFilter}
			/>
		</div>
		<SessionSidebar {session} {detail} {fileStats} />
	</div>
</div>
