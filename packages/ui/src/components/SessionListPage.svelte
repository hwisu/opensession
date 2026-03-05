<script lang="ts">
import { onMount } from 'svelte';
import { listSessionRepos, listSessions } from '../api';
import type { SessionSummary, TimeRange } from '../types';
import { TOOL_CONFIGS } from '../types';
import { sessionTitleFallback, stripTags } from '../utils';
import SessionCard from './SessionCard.svelte';
import FloatingJobStatus from './FloatingJobStatus.svelte';

const {
	onNavigate,
}: {
	onNavigate: (path: string) => void;
} = $props();

let sessions = $state<SessionSummary[]>([]);
let total = $state(0);
let loading = $state(false);
let forceRefreshing = $state(false);
let error = $state<string | null>(null);
let searchQuery = $state('');
let toolFilter = $state('');
let repoFilter = $state('');
let repoInput = $state('');
let timeRange = $state<TimeRange>('all');
let currentPage = $state(1);
let selectedIndex = $state(0);
let renderLimit = $state(20);
let searchInput: HTMLInputElement | undefined = $state();
let repoFilterInputEl: HTMLInputElement | undefined = $state();
let fetchRequestId = 0;
let knownRepos = $state<string[]>([]);
let copyFeedback = $state<string | null>(null);
let copyFeedbackTimer: number | null = null;
let hydratedFromQuery = false;
let lastResetFingerprint = $state<string | null>(null);

const perPage = 20;
const listCacheKey = 'opensession_public_list_cache_v1';
const listCacheTtlMs = 30_000;

type SessionListCacheEntry = {
	query: string;
	created_at: number;
	total: number;
	sessions: SessionSummary[];
};

const hasMore = $derived(currentPage * perPage < total);
const visibleSessions = $derived(sessions.slice(0, renderLimit));
const hasHiddenRendered = $derived(renderLimit < sessions.length);
const navigableSessions = $derived(visibleSessions);
const selectedSessionId = $derived(navigableSessions[selectedIndex]?.id ?? null);
const sessionOrder = $derived.by(() => {
	const order = new Map<string, number>();
	navigableSessions.forEach((session, idx) => {
		order.set(session.id, idx);
	});
	return order;
});
const floatingJobs = $derived.by(() => {
	if (!forceRefreshing) return [];
	return [
		{
			id: 'session-refresh',
			label: 'Refreshing sessions',
			detail: 'Background reindex is running. You can continue browsing.',
		},
	];
});

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

function currentListQueryFingerprint(page: number): string {
	return JSON.stringify({
		search: searchQuery || '',
		tool: toolFilter || '',
		git_repo_name: repoFilter,
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
		repoFilter.length === 0 &&
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

function clearListCache() {
	if (typeof window === 'undefined') return;
	try {
		localStorage.removeItem(listCacheKey);
	} catch {
		// Ignore storage failures.
	}
}

async function fetchSessions(reset = false, opts: { force?: boolean } = {}) {
	const forceRefresh = opts.force === true;
	const requestId = ++fetchRequestId;
	const targetPage = reset ? 1 : currentPage;
	const preserveVisibleSessions = reset && forceRefresh && sessions.length > 0;
	forceRefreshing = forceRefresh;

	let usedWarmCache = false;
	const fingerprint = currentListQueryFingerprint(targetPage);
	if (reset && !forceRefresh && lastResetFingerprint === fingerprint && sessions.length > 0) {
		return;
	}
	if (reset) {
		currentPage = targetPage;
		if (!preserveVisibleSessions) {
			sessions = [];
			selectedIndex = 0;
			renderLimit = perPage;
		}
	}
	if (reset && !forceRefresh && isDefaultPublicFeedQuery(targetPage)) {
		const cached = readListCache(fingerprint);
		if (cached) {
			sessions = cached.sessions;
			total = cached.total;
			renderLimit = Math.max(perPage, Math.min(cached.sessions.length, perPage));
			mergeKnownRepos(cached.sessions);
			usedWarmCache = true;
		}
	}

	loading = !usedWarmCache && !preserveVisibleSessions;
	error = null;
	try {
		const res = await listSessions({
			search: searchQuery || undefined,
			tool: toolFilter || undefined,
			git_repo_name: repoFilter || undefined,
			time_range: timeRange !== 'all' ? timeRange : undefined,
			page: targetPage,
			per_page: perPage,
			force_refresh: forceRefresh,
		});
		if (requestId !== fetchRequestId) return;
		if (reset) {
			sessions = res.sessions;
			renderLimit = Math.max(perPage, Math.min(res.sessions.length, perPage));
			lastResetFingerprint = fingerprint;
		} else {
			sessions = [...sessions, ...res.sessions];
		}
		total = res.total;
		mergeKnownRepos(res.sessions);
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
		if (forceRefresh) {
			forceRefreshing = false;
		}
	}
}

async function fetchKnownRepos() {
	try {
		const response = await listSessionRepos();
		knownRepos = [...response.repos].sort((a, b) => a.localeCompare(b));
	} catch {
		// Keep fallback behavior (derive from list payloads) when repo endpoint is unavailable.
	}
}

function handleSearch() {
	fetchSessions(true);
}

function forceRefreshSessions() {
	clearListCache();
	void fetchKnownRepos();
	fetchSessions(true, { force: true });
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
const validTimeRanges = new Set<TimeRange>(['all', '24h', '7d', '30d']);

function hydrateFiltersFromQuery() {
	if (typeof window === 'undefined') return;
	const params = new URLSearchParams(window.location.search);

	const repoFromQuery = params.get('git_repo_name')?.trim();
	if (repoFromQuery) {
		repoFilter = repoFromQuery;
		repoInput = repoFromQuery;
	}

	const searchFromQuery = params.get('search')?.trim();
	if (searchFromQuery) {
		searchQuery = searchFromQuery;
	}

	const toolFromQuery = params.get('tool')?.trim();
	if (toolFromQuery && tools.some((entry) => entry.value === toolFromQuery)) {
		toolFilter = toolFromQuery;
	}

	const rangeFromQuery = params.get('time_range')?.trim() as TimeRange | undefined;
	if (rangeFromQuery && validTimeRanges.has(rangeFromQuery)) {
		timeRange = rangeFromQuery;
	}
}

function extractRepos(items: SessionSummary[]): string[] {
	const values = new Set<string>();
	for (const session of items) {
		const repo = session.git_repo_name?.trim();
		if (repo) values.add(repo);
	}
	return [...values];
}

function mergeKnownRepos(items: SessionSummary[]) {
	const merged = new Set(knownRepos);
	for (const repo of extractRepos(items)) {
		merged.add(repo);
	}
	knownRepos = [...merged].sort((a, b) => a.localeCompare(b));
}

function applyRepoFilter(nextValue: string) {
	const normalized = nextValue.trim();
	repoFilter = normalized;
	repoInput = normalized;
	fetchSessions(true);
}

function clearRepoFilter() {
	applyRepoFilter('');
}

function cycleFilterValue<T extends string>(current: T, options: readonly T[]): T {
	const idx = options.indexOf(current);
	return options[(idx + 1) % options.length] ?? options[0];
}

function isSearchFocusShortcut(e: KeyboardEvent): boolean {
	if (e.key.toLowerCase() === 'f' && (e.metaKey || e.ctrlKey)) return true;
	return e.code === 'Slash' || e.key === '/';
}

function isEditableTarget(target: EventTarget | null): boolean {
	if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) return true;
	return target instanceof HTMLElement && target.isContentEditable;
}

function hasEditableSelection(target: EventTarget | null): boolean {
	if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) {
		const start = target.selectionStart ?? 0;
		const end = target.selectionEnd ?? 0;
		return end > start;
	}
	if (target instanceof HTMLElement && target.isContentEditable) {
		const selection = window.getSelection();
		return (selection?.toString() ?? '').trim().length > 0;
	}
	return false;
}

function focusSearchInput() {
	searchInput?.focus();
	searchInput?.select();
}

function focusRepoFilterInput() {
	repoFilterInputEl?.focus();
	repoFilterInputEl?.select();
}

function isCopyShortcut(e: KeyboardEvent): boolean {
	return (e.metaKey || e.ctrlKey) && !e.altKey && e.key.toLowerCase() === 'c';
}

function selectedSessionTitleForCopy(): string {
	const selected = navigableSessions[selectedIndex];
	if (!selected) return '';
	const title = stripTags(selected.title ?? '').trim();
	if (title.length > 0) return title;
	const description = stripTags(selected.description ?? '').trim();
	if (description.length > 0) return description;
	return sessionTitleFallback(selected.id);
}

function setCopyFeedbackMessage(message: string | null) {
	copyFeedback = message;
	if (copyFeedbackTimer != null) {
		window.clearTimeout(copyFeedbackTimer);
		copyFeedbackTimer = null;
	}
	if (!message) return;
	copyFeedbackTimer = window.setTimeout(() => {
		copyFeedback = null;
		copyFeedbackTimer = null;
	}, 1200);
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

async function handleCopyShortcut(e: KeyboardEvent) {
	const selectedText = window.getSelection()?.toString() ?? '';
	const fallbackText = selectedSessionTitleForCopy();
	const textToCopy = selectedText.trim().length > 0 ? selectedText : fallbackText;
	if (!textToCopy) return;
	e.preventDefault();
	const copied = await writeClipboardText(textToCopy);
	setCopyFeedbackMessage(copied ? 'Copied' : 'Copy failed');
}

async function copySelectedSessionTitle() {
	const text = selectedSessionTitleForCopy();
	if (!text) return;
	const copied = await writeClipboardText(text);
	setCopyFeedbackMessage(copied ? 'Copied' : 'Copy failed');
}

onMount(() => {
	if (!hydratedFromQuery) {
		hydrateFiltersFromQuery();
		hydratedFromQuery = true;
	}
	void fetchKnownRepos();
	void fetchSessions(true);
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

$effect(() => {
	return () => {
		if (copyFeedbackTimer != null) {
			window.clearTimeout(copyFeedbackTimer);
			copyFeedbackTimer = null;
		}
	};
});

function handleKeydown(e: KeyboardEvent) {
	if (isCopyShortcut(e)) {
		if (hasEditableSelection(e.target)) return;
		void handleCopyShortcut(e);
		return;
	}
	if (isEditableTarget(e.target)) return;
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
	} else if (e.key.toLowerCase() === 'g') {
		e.preventDefault();
		focusRepoFilterInput();
	} else if (e.key === 't') {
		e.preventDefault();
		const toolValues = tools.map((t) => t.value);
		toolFilter = cycleFilterValue(toolFilter, toolValues);
		fetchSessions(true);
	} else if (e.key === 'r') {
		e.preventDefault();
		timeRange = cycleFilterValue(timeRange, rangeCycle);
		fetchSessions(true);
	} else if (e.key === 'R') {
		e.preventDefault();
		forceRefreshSessions();
	}
}

function handleCopyEvent(e: ClipboardEvent) {
	if (hasEditableSelection(e.target)) return;
	const selectedText = window.getSelection()?.toString() ?? '';
	const fallbackText = selectedSessionTitleForCopy();
	const textToCopy = selectedText.trim().length > 0 ? selectedText : fallbackText;
	if (!textToCopy) return;
	e.preventDefault();
	if (e.clipboardData) {
		e.clipboardData.setData('text/plain', textToCopy);
		setCopyFeedbackMessage('Copied');
		return;
	}
	void copySelectedSessionTitle();
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

function handleRepoInputKeydown(e: KeyboardEvent) {
	if (e.key === 'Enter') {
		e.preventDefault();
		applyRepoFilter(repoInput);
		return;
	}
	if (e.key === 'Escape') {
		e.preventDefault();
		if (repoInput.trim().length > 0 || repoFilter.length > 0) {
			clearRepoFilter();
			return;
		}
		(e.target as HTMLInputElement | null)?.blur();
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

<svelte:window oncopy={handleCopyEvent} onkeydown={handleKeydown} />

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
		<div class="flex w-full items-center gap-1 sm:w-auto">
			<label for="session-repo-filter" class="shrink-0 text-xs text-text-muted">repo</label>
			<input
				id="session-repo-filter"
				list="session-repo-options"
				type="text"
				placeholder="org/repo"
				bind:this={repoFilterInputEl}
				bind:value={repoInput}
				onkeydown={handleRepoInputKeydown}
				onblur={() => applyRepoFilter(repoInput)}
				class="w-full min-w-0 border border-border bg-bg-secondary px-2 py-0.5 text-xs text-text-secondary outline-none focus:border-accent sm:w-48"
			/>
			<datalist id="session-repo-options">
				{#each knownRepos as repo}
					<option value={repo}></option>
				{/each}
			</datalist>
			{#if repoFilter}
				<button
					type="button"
					onclick={clearRepoFilter}
					class="shrink-0 border border-border bg-bg-secondary px-1.5 py-0.5 text-xs text-text-muted transition-colors hover:text-text-primary"
				>
					clear
				</button>
			{/if}
		</div>
		<div
			data-testid="list-shortcut-legend"
			class="flex w-full flex-wrap items-center gap-1 text-[11px] text-text-muted"
		>
			<span class="inline-flex items-center gap-1 rounded border border-border bg-bg-secondary px-1.5 py-0.5">
				<kbd class="rounded border border-accent/40 bg-accent/10 px-1 py-[1px] font-mono text-[10px] text-accent">Cmd/Ctrl+C</kbd>
				<span>copy title</span>
			</span>
			<span class="inline-flex items-center gap-1 rounded border border-border bg-bg-secondary px-1.5 py-0.5">
				<kbd class="rounded border border-accent/40 bg-accent/10 px-1 py-[1px] font-mono text-[10px] text-accent">Shift+R</kbd>
				<span>force refresh</span>
			</span>
			{#if copyFeedback}
				<span
					data-testid="session-copy-feedback"
					class="rounded border border-border bg-bg-secondary px-1.5 py-0.5"
					class:text-success={copyFeedback === 'Copied'}
					class:text-error={copyFeedback === 'Copy failed'}
				>
					{copyFeedback}
				</span>
			{/if}
			<button
				type="button"
				onclick={copySelectedSessionTitle}
				class="rounded border border-border bg-bg-secondary px-1.5 py-0.5 text-[11px] text-text-secondary transition-colors hover:text-text-primary"
			>
				Copy selected
			</button>
			<button
				type="button"
				data-testid="session-force-refresh"
				onclick={forceRefreshSessions}
				disabled={forceRefreshing}
				class="rounded border border-border bg-bg-secondary px-1.5 py-0.5 text-[11px] text-text-secondary transition-colors hover:text-text-primary disabled:opacity-60"
			>
				{forceRefreshing ? 'Refreshing...' : 'Force refresh'}
			</button>
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
			<span class="ml-2 text-text-secondary">[single chronological feed]</span>
		</div>

			{#if sessions.length === 0 && !loading}
				<div class="py-16 text-center">
					<p class="text-sm text-text-muted">No sessions found</p>
					<p class="mt-1 text-xs text-text-muted">Public feed is currently empty.</p>
				</div>
			{/if}

		<div>
			{#each visibleSessions as session (session.id)}
				<div data-session-idx={sessionIndex(session.id)} data-session-id={session.id}>
					<SessionCard session={session} selected={selectedSessionId === session.id} />
				</div>
			{/each}
		</div>

		{#if loading}
			<div class="py-4 text-center text-xs text-text-muted">
				{sessions.length === 0 ? 'Loading...' : 'Updating...'}
			</div>
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

<FloatingJobStatus jobs={floatingJobs} />
