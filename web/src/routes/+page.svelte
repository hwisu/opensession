<script lang="ts">
	import SessionCard from '$lib/components/SessionCard.svelte';
	import { listSessions } from '$lib/api';
	import type { SessionListItem } from '$lib/types';
	import { TOOL_CONFIGS } from '$lib/types';

	let sessions = $state<SessionListItem[]>([]);
	let total = $state(0);
	let loading = $state(false);
	let error = $state<string | null>(null);
	let searchQuery = $state('');
	let toolFilter = $state('');
	let sortBy = $state('recent');
	let timeRange = $state('all');
	let currentPage = $state(1);
	const perPage = 20;

	let hasMore = $derived(currentPage * perPage < total);

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
				per_page: perPage
			});
			if (reset) {
				sessions = res.sessions;
			} else {
				sessions = [...sessions, ...res.sessions];
			}
			total = res.total;
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
		...Object.values(TOOL_CONFIGS).map((t) => ({ value: t.name, label: t.label }))
	];

	$effect(() => {
		fetchSessions(true);
	});
</script>

<svelte:head>
	<title>opensession.io - AI Session Explorer</title>
</svelte:head>

<div>
	<!-- Header with time filter tabs -->
	<div class="mb-6 flex items-end justify-between">
		<div>
			<h1 class="text-2xl font-bold text-white">Session Feed</h1>
			<p class="mt-1 text-sm text-text-secondary">
				Explore AI coding sessions from the community
			</p>
		</div>
		<div class="flex rounded-lg border border-border bg-bg-secondary p-0.5">
			{#each [
				{ value: 'all', label: 'All Time' },
				{ value: '24h', label: '24h' },
				{ value: '7d', label: '7d' },
				{ value: '30d', label: '30d' }
			] as tab}
				<button
					onclick={() => { timeRange = tab.value; fetchSessions(true); }}
					class="rounded-md px-3 py-1 text-xs font-medium transition-colors
						{timeRange === tab.value
						? 'bg-accent text-white'
						: 'text-text-secondary hover:text-text-primary'}"
				>
					{tab.label}
				</button>
			{/each}
		</div>
	</div>

	<!-- Search + filters -->
	<div class="mb-4 flex flex-col gap-3 sm:flex-row">
		<div class="flex-1">
			<input
				type="text"
				placeholder="Search sessions..."
				bind:value={searchQuery}
				onkeydown={(e) => e.key === 'Enter' && handleSearch()}
				class="w-full rounded-lg border border-border bg-bg-secondary px-3 py-2 text-sm text-text-primary placeholder-text-muted outline-none transition-colors focus:border-accent"
			/>
		</div>
		<select
			bind:value={toolFilter}
			onchange={() => fetchSessions(true)}
			class="rounded-lg border border-border bg-bg-secondary px-3 py-2 text-sm text-text-secondary outline-none transition-colors focus:border-accent"
		>
			{#each tools as t}
				<option value={t.value}>{t.label}</option>
			{/each}
		</select>
		<select
			bind:value={sortBy}
			onchange={() => fetchSessions(true)}
			class="rounded-lg border border-border bg-bg-secondary px-3 py-2 text-sm text-text-secondary outline-none transition-colors focus:border-accent"
		>
			<option value="recent">Recent</option>
			<option value="popular">Most Messages</option>
			<option value="longest">Longest</option>
		</select>
		<button
			onclick={handleSearch}
			class="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-accent/80"
		>
			Search
		</button>
	</div>

	{#if error}
		<div class="mb-4 rounded-lg border border-error/30 bg-error/10 px-4 py-3 text-sm text-error">
			{error}
		</div>
	{/if}

	{#if sessions.length === 0 && !loading}
		<div class="py-16 text-center">
			<p class="text-lg text-text-muted">No sessions found</p>
			<p class="mt-1 text-sm text-text-muted">
				<a href="/upload" class="text-accent hover:underline">Upload</a> a session to get started
			</p>
		</div>
	{/if}

	<div class="grid gap-3">
		{#each sessions as session (session.id)}
			<SessionCard {session} />
		{/each}
	</div>

	{#if loading}
		<div class="py-8 text-center text-sm text-text-muted">Loading...</div>
	{/if}

	{#if hasMore && !loading}
		<div class="mt-4 text-center">
			<button
				onclick={loadMore}
				class="rounded-lg border border-border px-6 py-2 text-sm text-text-secondary transition-colors hover:border-border-light hover:text-text-primary"
			>
				Load More
			</button>
		</div>
	{/if}
</div>
