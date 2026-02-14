<script lang="ts">
import { listSessions } from '../api';
import type { SessionListItem, SortOrder, TimeRange } from '../types';
import { TOOL_CONFIGS } from '../types';
import SessionCard from './SessionCard.svelte';

const { onNavigate }: { onNavigate: (path: string) => void } = $props();

let sessions = $state<SessionListItem[]>([]);
let total = $state(0);
let loading = $state(false);
let error = $state<string | null>(null);
let searchQuery = $state('');
let toolFilter = $state('');
let sortBy = $state<SortOrder>('recent');
let timeRange = $state<TimeRange>('all');
let currentPage = $state(1);
const perPage = 20;

const hasMore = $derived(currentPage * perPage < total);
let selectedIndex = $state(0);
let searchInput: HTMLInputElement | undefined = $state();
const sortCycle: readonly SortOrder[] = ['recent', 'popular', 'longest'];
const rangeCycle: readonly TimeRange[] = ['all', '24h', '7d', '30d'];
const timeRangeTabs: ReadonlyArray<{ value: TimeRange; label: string }> = [
	{ value: 'all', label: 'All Time' },
	{ value: '24h', label: '24h' },
	{ value: '7d', label: '7d' },
	{ value: '30d', label: '30d' },
];

async function fetchSessions(reset = false) {
	if (reset) {
		currentPage = 1;
		sessions = [];
	}
	loading = true;
	error = null;
	try {
		const res = await listSessions({
			search: searchQuery || undefined,
			tool: toolFilter || undefined,
			sort: sortBy !== 'recent' ? sortBy : undefined,
			time_range: timeRange !== 'all' ? timeRange : undefined,
			page: currentPage,
			per_page: perPage,
		});
		if (reset) {
			sessions = res.sessions;
		} else {
			sessions = [...sessions, ...res.sessions];
		}
		total = res.total;
		if (reset) selectedIndex = 0;
	} catch (e) {
		error = e instanceof Error ? e.message : 'Failed to load sessions';
	} finally {
		loading = false;
	}
}

function handleSearch() {
	fetchSessions(true);
}

function loadMore() {
	currentPage += 1;
	fetchSessions(false);
}

const tools = [
	{ value: '', label: 'All Tools' },
	...Object.values(TOOL_CONFIGS).map((t) => ({ value: t.name, label: t.label })),
];

function cycleFilterValue<T extends string>(current: T, options: readonly T[]): T {
	const idx = options.indexOf(current);
	return options[(idx + 1) % options.length] ?? options[0];
}

$effect(() => {
	fetchSessions(true);
});

function handleKeydown(e: KeyboardEvent) {
	if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
	if (e.key === 'j') {
		e.preventDefault();
		if (selectedIndex < sessions.length - 1) selectedIndex++;
		scrollSelectedIntoView();
	} else if (e.key === 'k') {
		e.preventDefault();
		if (selectedIndex > 0) selectedIndex--;
		scrollSelectedIntoView();
	} else if (e.key === 'Enter') {
		e.preventDefault();
		if (sessions[selectedIndex]) {
			onNavigate(`/session/${sessions[selectedIndex].id}`);
		}
	} else if (e.key === '/') {
		e.preventDefault();
		searchInput?.focus();
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
	}
}

function scrollSelectedIntoView() {
	const el = document.querySelector(`[data-session-idx="${selectedIndex}"]`);
	el?.scrollIntoView({ block: 'nearest' });
}
</script>

<svelte:window onkeydown={handleKeydown} />

<svelte:head>
	<title>opensession.io - AI Session Explorer</title>
</svelte:head>

<div class="flex h-full flex-col">
	<!-- Filter bar -->
	<div class="flex shrink-0 flex-wrap items-center gap-3 border-b border-border px-2 py-1.5">
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

		<div class="flex flex-1 items-center gap-1">
			<label for="session-search" class="text-xs text-text-muted">/</label>
			<input
				id="session-search"
				type="text"
				placeholder="search..."
				bind:this={searchInput}
				bind:value={searchQuery}
				onkeydown={(e) => e.key === 'Enter' && handleSearch()}
				class="w-full min-w-0 border-none bg-transparent px-1 py-0.5 text-xs text-text-primary placeholder-text-muted outline-none"
			/>
		</div>

		<select
			bind:value={toolFilter}
			onchange={() => fetchSessions(true)}
			class="border border-border bg-bg-secondary px-2 py-0.5 text-xs text-text-secondary outline-none focus:border-accent"
		>
			{#each tools as t}
				<option value={t.value}>{t.label}</option>
			{/each}
		</select>
		<select
			bind:value={sortBy}
			onchange={() => fetchSessions(true)}
			class="border border-border bg-bg-secondary px-2 py-0.5 text-xs text-text-secondary outline-none focus:border-accent"
		>
			<option value="recent">Recent</option>
			<option value="popular">Most Messages</option>
			<option value="longest">Longest</option>
		</select>
	</div>

	{#if error}
		<div class="border-b border-error/30 bg-error/10 px-4 py-2 text-xs text-error">
			{error}
		</div>
	{/if}

	<div class="flex-1 overflow-y-auto">
		<div class="border-b border-border px-3 py-1 text-xs text-text-muted">
			Sessions ({total})
		</div>

		{#if sessions.length === 0 && !loading}
			<div class="py-16 text-center">
				<p class="text-sm text-text-muted">No sessions found</p>
				<p class="mt-1 text-xs text-text-muted">
					<a href="/upload" class="text-accent hover:underline">Upload</a> a session to get started
				</p>
			</div>
		{/if}

		<div>
			{#each sessions as session, idx (session.id)}
				<div data-session-idx={idx}>
					<SessionCard {session} selected={selectedIndex === idx} />
				</div>
			{/each}
		</div>

		{#if loading}
			<div class="py-4 text-center text-xs text-text-muted">Loading...</div>
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
