<script lang="ts">
import { listSessions } from '../api';
import { groupSessionsByAgentCount } from '../session-presentation';
import type { SessionSummary, SortOrder, TimeRange } from '../types';
import { TOOL_CONFIGS } from '../types';
import SessionCard from './SessionCard.svelte';

const {
	onNavigate,
}: {
	onNavigate: (path: string) => void;
} = $props();

type ListLayout = 'single' | 'agent-columns';

let sessions = $state<SessionSummary[]>([]);
let total = $state(0);
let loading = $state(false);
let error = $state<string | null>(null);
let searchQuery = $state('');
let toolFilter = $state('');
let sortBy = $state<SortOrder>('recent');
let timeRange = $state<TimeRange>('all');
let currentPage = $state(1);
let selectedIndex = $state(0);
let listLayout = $state<ListLayout>('single');
let renderLimit = $state(20);
let searchInput: HTMLInputElement | undefined = $state();
let fetchRequestId = 0;

const perPage = 20;
const layoutPreferenceKey = 'opensession_session_list_layout';
const listCacheKey = 'opensession_public_list_cache_v1';
const listCacheTtlMs = 30_000;
const layoutTabs: ReadonlyArray<{
	value: ListLayout;
	label: string;
	title: string;
}> = [
	{
		value: 'single',
		label: 'List',
		title: 'Single feed across all sessions',
	},
	{
		value: 'agent-columns',
		label: 'Agents',
		title: 'Group sessions by max active agents (parallel lanes)',
	},
];

type SessionListCacheEntry = {
	query: string;
	created_at: number;
	total: number;
	sessions: SessionSummary[];
};

const hasMore = $derived(currentPage * perPage < total);
const visibleSessions = $derived(sessions.slice(0, renderLimit));
const hasHiddenRendered = $derived(renderLimit < sessions.length);
const groupedByAgents = $derived(groupSessionsByAgentCount(visibleSessions));
const visibleColumnCount = $derived(groupedByAgents.length);
const navigableSessions = $derived(visibleSessions);
const selectedSessionId = $derived(navigableSessions[selectedIndex]?.id ?? null);
const layoutSummaryLabel = $derived(
	listLayout === 'single'
		? 'List = one chronological feed'
		: 'Agents = grouped by max active agents',
);
const layoutSummaryDetail = $derived(
	listLayout === 'single'
		? 'Best for scanning overall flow with selected order.'
		: 'Best for seeing parallelism and handoff density.',
);
const sessionOrder = $derived.by(() => {
	const order = new Map<string, number>();
	navigableSessions.forEach((session, idx) => {
		order.set(session.id, idx);
	});
	return order;
});

const sortCycle: readonly SortOrder[] = ['recent', 'popular', 'longest'];
const rangeCycle: readonly TimeRange[] = ['all', '24h', '7d', '30d'];
const timeRangeTabs: ReadonlyArray<{ value: TimeRange; label: string }> = [
	{ value: 'all', label: 'All Time' },
	{ value: '24h', label: '24h' },
	{ value: '7d', label: '7d' },
	{ value: '30d', label: '30d' },
];

function sessionIndex(sessionId: string): number {
	return sessionOrder.get(sessionId) ?? -1;
}

function syncLayoutPreference() {
	if (typeof window === 'undefined') return;
	const stored = localStorage.getItem(layoutPreferenceKey);
	if (stored === 'agent-columns' || stored === 'single') {
		listLayout = stored;
	}
}

function persistLayoutPreference() {
	if (typeof window === 'undefined') return;
	localStorage.setItem(layoutPreferenceKey, listLayout);
}

function toggleLayout() {
	listLayout = listLayout === 'single' ? 'agent-columns' : 'single';
}

function currentListQueryFingerprint(page: number): string {
	return JSON.stringify({
		search: searchQuery || '',
		tool: toolFilter || '',
		sort: sortBy,
		time_range: timeRange,
		page,
		per_page: perPage,
	});
}

function isDefaultPublicFeedQuery(page: number): boolean {
	return (
		page === 1 &&
		searchQuery.trim().length === 0 &&
		toolFilter.length === 0 &&
		sortBy === 'recent' &&
		timeRange === 'all'
	);
}

function readListCache(fingerprint: string): SessionListCacheEntry | null {
	if (typeof window === 'undefined') return null;
	try {
		const raw = localStorage.getItem(listCacheKey);
		if (!raw) return null;
		const parsed = JSON.parse(raw) as SessionListCacheEntry;
		if (!parsed || parsed.query !== fingerprint) return null;
		if (Date.now() - parsed.created_at > listCacheTtlMs) return null;
		if (!Array.isArray(parsed.sessions)) return null;
		return parsed;
	} catch {
		return null;
	}
}

function writeListCache(entry: SessionListCacheEntry) {
	if (typeof window === 'undefined') return;
	try {
		localStorage.setItem(listCacheKey, JSON.stringify(entry));
	} catch {
		// Ignore storage quota/private mode errors.
	}
}

async function fetchSessions(reset = false) {
	const requestId = ++fetchRequestId;
	const targetPage = reset ? 1 : currentPage;

	if (reset) {
		currentPage = targetPage;
		sessions = [];
		selectedIndex = 0;
		renderLimit = perPage;
	}

	let usedWarmCache = false;
	const fingerprint = currentListQueryFingerprint(targetPage);
	if (reset && isDefaultPublicFeedQuery(targetPage)) {
		const cached = readListCache(fingerprint);
		if (cached) {
			sessions = cached.sessions;
			total = cached.total;
			renderLimit = Math.max(perPage, Math.min(cached.sessions.length, perPage));
			usedWarmCache = true;
		}
	}

	loading = !usedWarmCache;
	error = null;
	try {
		const res = await listSessions({
			search: searchQuery || undefined,
			tool: toolFilter || undefined,
			sort: sortBy !== 'recent' ? sortBy : undefined,
			time_range: timeRange !== 'all' ? timeRange : undefined,
			page: targetPage,
			per_page: perPage,
		});
		if (requestId !== fetchRequestId) return;
		if (reset) {
			sessions = res.sessions;
			renderLimit = Math.max(perPage, Math.min(res.sessions.length, perPage));
		} else {
			sessions = [...sessions, ...res.sessions];
		}
		total = res.total;
		if (reset && isDefaultPublicFeedQuery(targetPage)) {
			writeListCache({
				query: fingerprint,
				created_at: Date.now(),
				total: res.total,
				sessions: res.sessions,
			});
		}
	} catch (e) {
		if (requestId !== fetchRequestId) return;
		error = e instanceof Error ? e.message : 'Failed to load sessions';
	} finally {
		if (requestId === fetchRequestId) {
			loading = false;
		}
	}
}

function handleSearch() {
	fetchSessions(true);
}

function loadMore() {
	currentPage += 1;
	fetchSessions(false);
}

function renderMore() {
	renderLimit = Math.min(renderLimit + perPage, sessions.length);
}

const tools = [
	{ value: '', label: 'All Tools' },
	...Object.values(TOOL_CONFIGS).map((t) => ({ value: t.name, label: t.label })),
];

function cycleFilterValue<T extends string>(current: T, options: readonly T[]): T {
	const idx = options.indexOf(current);
	return options[(idx + 1) % options.length] ?? options[0];
}

function isSearchFocusShortcut(e: KeyboardEvent): boolean {
	if (e.key.toLowerCase() === 'f' && (e.metaKey || e.ctrlKey)) return true;
	return e.code === 'Slash' || e.key === '/' || e.key === '?';
}

function focusSearchInput() {
	searchInput?.focus();
	searchInput?.select();
}

$effect(() => {
	syncLayoutPreference();
	fetchSessions(true);
});

$effect(() => {
	persistLayoutPreference();
});

$effect(() => {
	const len = navigableSessions.length;
	if (len === 0) {
		selectedIndex = 0;
		return;
	}
	if (selectedIndex >= len) {
		selectedIndex = len - 1;
	}
});

function handleKeydown(e: KeyboardEvent) {
	if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
	if (e.key === 'j') {
		e.preventDefault();
		if (selectedIndex < navigableSessions.length - 1) selectedIndex++;
		scrollSelectedIntoView();
	} else if (e.key === 'k') {
		e.preventDefault();
		if (selectedIndex > 0) selectedIndex--;
		scrollSelectedIntoView();
	} else if (e.key === 'Enter') {
		e.preventDefault();
		const selected = navigableSessions[selectedIndex];
		if (selected) {
			onNavigate(`/session/${selected.id}`);
		}
	} else if (isSearchFocusShortcut(e)) {
		e.preventDefault();
		focusSearchInput();
	} else if (e.key === 't') {
		e.preventDefault();
		const toolValues = tools.map((t) => t.value);
		toolFilter = cycleFilterValue(toolFilter, toolValues);
		fetchSessions(true);
	} else if (e.key === 'o') {
		e.preventDefault();
		sortBy = cycleFilterValue(sortBy, sortCycle);
		fetchSessions(true);
	} else if (e.key === 'r') {
		e.preventDefault();
		timeRange = cycleFilterValue(timeRange, rangeCycle);
		fetchSessions(true);
	} else if (e.key === 'l') {
		e.preventDefault();
		toggleLayout();
	}
}

function handleSearchInputKeydown(e: KeyboardEvent) {
	if (e.key === 'Enter') {
		e.preventDefault();
		handleSearch();
		return;
	}

	if (e.key === 'Escape') {
		e.preventDefault();
		if (searchQuery.trim().length > 0) {
			searchQuery = '';
			handleSearch();
			return;
		}
		searchInput?.blur();
	}
}

function scrollSelectedIntoView() {
	const el = document.querySelector(`[data-session-idx="${selectedIndex}"]`);
	el?.scrollIntoView({ block: 'nearest' });
}

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
</script>

<svelte:window onkeydown={handleKeydown} />

<svelte:head>
	<title>opensession.io - AI Session Explorer</title>
</svelte:head>

<div class="flex h-full flex-col">
	<div class="flex shrink-0 flex-wrap items-center gap-2 border-b border-border px-2 py-1.5">
		<div class="flex items-center gap-1" role="tablist" aria-label="Time range">
			{#each timeRangeTabs as tab}
				<button
					role="tab"
					aria-selected={timeRange === tab.value}
					onclick={() => { timeRange = tab.value; fetchSessions(true); }}
					class="px-2 py-0.5 text-xs transition-colors
						{timeRange === tab.value
						? 'bg-accent text-white'
						: 'text-text-secondary hover:text-text-primary'}"
				>
					{timeRange === tab.value ? `[${tab.label}]` : tab.label}
				</button>
			{/each}
		</div>

		<div class="order-3 flex w-full items-center gap-1 sm:order-none sm:flex-1">
			<label for="session-search" class="text-xs text-text-muted">/</label>
			<input
				id="session-search"
				type="text"
				placeholder="search..."
				bind:this={searchInput}
				bind:value={searchQuery}
				onkeydown={handleSearchInputKeydown}
				class="w-full min-w-0 border-none bg-transparent px-1 py-0.5 text-xs text-text-primary placeholder-text-muted outline-none"
			/>
		</div>

		<select
			bind:value={toolFilter}
			onchange={() => fetchSessions(true)}
			class="w-full border border-border bg-bg-secondary px-2 py-0.5 text-xs text-text-secondary outline-none focus:border-accent sm:w-auto"
		>
			{#each tools as t}
				<option value={t.value}>{t.label}</option>
			{/each}
		</select>
		<select
			bind:value={sortBy}
			onchange={() => fetchSessions(true)}
			class="w-full border border-border bg-bg-secondary px-2 py-0.5 text-xs text-text-secondary outline-none focus:border-accent sm:w-auto"
		>
			<option value="recent">Recent</option>
			<option value="popular">Most Messages</option>
			<option value="longest">Longest</option>
		</select>
		<div class="flex w-full items-center justify-center border border-border bg-bg-secondary p-0.5 sm:w-auto" role="tablist" aria-label="Session layout">
			{#each layoutTabs as tab}
				<button
					role="tab"
					aria-selected={listLayout === tab.value}
					onclick={() => { listLayout = tab.value; }}
					title={tab.title}
					class="px-2 py-0.5 text-xs"
					class:bg-bg-hover={listLayout === tab.value}
				>
					{tab.label}
				</button>
			{/each}
		</div>
		<div
			data-testid="list-shortcut-legend"
			class="flex w-full flex-wrap items-center gap-1 text-[11px] text-text-muted"
		>
			<span class="inline-flex items-center gap-1 rounded border border-border bg-bg-secondary px-1.5 py-0.5">
				<kbd class="rounded border border-accent/40 bg-accent/10 px-1 py-[1px] font-mono text-[10px] text-accent">t</kbd>
				<span>tool</span>
			</span>
			<span class="inline-flex items-center gap-1 rounded border border-border bg-bg-secondary px-1.5 py-0.5">
				<kbd class="rounded border border-accent/40 bg-accent/10 px-1 py-[1px] font-mono text-[10px] text-accent">o</kbd>
				<span>order</span>
			</span>
			<span class="inline-flex items-center gap-1 rounded border border-border bg-bg-secondary px-1.5 py-0.5">
				<kbd class="rounded border border-accent/40 bg-accent/10 px-1 py-[1px] font-mono text-[10px] text-accent">r</kbd>
				<span>range</span>
			</span>
			<span class="text-text-secondary">|</span>
			<span class="text-text-secondary">
				<kbd class="rounded border border-accent/40 bg-accent/10 px-1 py-[1px] font-mono text-[10px] text-accent">l</kbd>
				layout
			</span>
		</div>
	</div>

	{#if error}
		<div class="border-b border-error/30 bg-error/10 px-4 py-2 text-xs text-error">
			{error}
		</div>
	{/if}

	<div class="flex-1 overflow-y-auto">
		<div data-testid="session-layout-summary" class="border-b border-border px-3 py-1 text-xs text-text-muted">
			Sessions ({total})
			<span class="ml-2 text-text-secondary">[{layoutSummaryLabel}]</span>
			<span class="ml-2 text-text-secondary">[cols:{visibleColumnCount}]</span>
			<span class="ml-2 text-text-secondary">{layoutSummaryDetail}</span>
		</div>

			{#if sessions.length === 0 && !loading}
				<div class="py-16 text-center">
					<p class="text-sm text-text-muted">No sessions found</p>
					<p class="mt-1 text-xs text-text-muted">Public feed is currently empty.</p>
				</div>
			{/if}

		{#if listLayout === 'single'}
			<div>
				{#each visibleSessions as session (session.id)}
					<div data-session-idx={sessionIndex(session.id)} data-session-id={session.id}>
						<SessionCard session={session} selected={selectedSessionId === session.id} />
					</div>
				{/each}
			</div>
		{:else}
			<div class="flex gap-3 overflow-x-auto px-2 py-2">
				{#each groupedByAgents as group}
					<section
						class={`flex-1 border border-border bg-bg-secondary/30 ${
							visibleColumnCount > 1 ? 'min-w-[300px] max-w-[420px]' : 'min-w-0'
						}`}
					>
						<header class="flex items-center justify-between border-b border-border px-2 py-1 text-xs">
							<span class="font-semibold" style="color: {group.color};">{group.label}</span>
							<span class="text-text-muted">{group.sessions.length}</span>
						</header>
						<div>
							{#each group.sessions as session (session.id)}
								<div data-session-idx={sessionIndex(session.id)} data-session-id={session.id}>
									<SessionCard
										session={session}
										selected={selectedSessionId === session.id}
										compact={true}
									/>
								</div>
							{/each}
						</div>
					</section>
				{/each}
			</div>
		{/if}

		{#if loading}
			<div class="py-4 text-center text-xs text-text-muted">Loading...</div>
		{/if}

		{#if hasHiddenRendered && !loading}
			<div class="border-t border-border py-2 text-center">
				<button
					onclick={renderMore}
					class="px-4 py-1 text-xs text-text-secondary transition-colors hover:text-text-primary"
				>
					Render More
				</button>
			</div>
		{/if}

		{#if hasMore && !loading}
			<div class="border-t border-border py-2 text-center">
				<button
					onclick={loadMore}
					class="px-4 py-1 text-xs text-text-secondary transition-colors hover:text-text-primary"
				>
					Load More
				</button>
			</div>
		{/if}
	</div>
</div>
