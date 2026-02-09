<script lang="ts">
	import { page } from '$app/stores';
	import { getSession } from '$lib/api';
	import { getToolConfig, formatDuration, formatTimestamp } from '$lib/types';
	import type { SessionDetail, Session } from '$lib/types';
	import TimelineView from '$lib/components/TimelineView.svelte';
	import { SessionSidebar } from '@opensession/ui/components';
	import { getDisplayTitle, computeFileStats } from '@opensession/ui';

	let session = $state<Session | null>(null);
	let detail = $state<SessionDetail | null>(null);
	let loading = $state(true);
	let error = $state<string | null>(null);

	let tool = $derived(session ? getToolConfig(session.agent.tool) : null);
	let displayTitle = $derived(session ? getDisplayTitle(session) : 'Session');
	let fileStats = $derived(session ? computeFileStats(session.events) : { filesChanged: 0, linesAdded: 0, linesRemoved: 0 });

	async function fetchDetail(id: string): Promise<SessionDetail> {
		const baseUrl = window.location.origin;
		const res = await fetch(`${baseUrl}/api/sessions/${encodeURIComponent(id)}`);
		if (!res.ok) throw new Error('Failed to load session detail');
		return res.json();
	}

	$effect(() => {
		const id = $page.params.id!;
		loading = true;
		error = null;
		Promise.all([
			getSession(id),
			fetchDetail(id),
		])
			.then(([s, d]) => {
				session = s;
				detail = d;
			})
			.catch((e) => {
				error = e instanceof Error ? e.message : 'Failed to load session';
			})
			.finally(() => {
				loading = false;
			});
	});
</script>

<svelte:head>
	<title>{displayTitle} - opensession</title>
</svelte:head>

{#if loading}
	<div class="py-16 text-center text-sm text-text-muted">Loading session...</div>
{:else if error}
	<div class="py-16 text-center">
		<p class="text-error">{error}</p>
		<a href="/" class="mt-2 inline-block text-sm text-accent hover:underline">Back to feed</a>
	</div>
{:else if session && tool}
	<div>
		<a href="/" class="mb-4 inline-block text-sm text-text-muted hover:text-text-secondary">
			&larr; Back to feed
		</a>

		<!-- Title + Author -->
		<div class="mb-6">
			<h1 class="text-2xl font-bold text-text-primary leading-tight">
				{displayTitle}
			</h1>
			<div class="mt-2 flex items-center gap-3">
				<div class="relative shrink-0">
					<div class="flex h-8 w-8 items-center justify-center rounded-full bg-bg-hover text-xs font-bold text-text-secondary">
						{(detail?.nickname ?? '?')[0].toUpperCase()}
					</div>
					<div
						class="absolute -bottom-1 -right-1 flex h-[18px] w-[18px] items-center justify-center rounded text-[9px] font-bold text-white ring-2 ring-bg-primary"
						style="background-color: {tool.color}"
					>
						{tool.icon}
					</div>
				</div>
				<span class="text-sm text-text-secondary">
					{detail?.nickname ?? 'anonymous'} &middot;
					{tool.label} &middot; {session.agent.model}
					{#if session.agent.tool_version}
						&middot; v{session.agent.tool_version}
					{/if}
				</span>
			</div>
		</div>

		<!-- Mobile: inline metadata -->
		<div class="mb-4 lg:hidden">
			<div class="flex flex-wrap gap-3 text-xs text-text-muted">
				<span>{formatTimestamp(session.context.created_at)}</span>
				<span>&middot;</span>
				<span>{session.stats.message_count} msgs</span>
				<span>&middot;</span>
				<span>{session.stats.tool_call_count} tools</span>
				<span>&middot;</span>
				<span>{formatDuration(session.stats.duration_seconds)}</span>
				{#if fileStats.filesChanged > 0}
					<span>&middot;</span>
					<span>{fileStats.filesChanged} files</span>
				{/if}
			</div>
		</div>

		<!-- Two-column layout -->
		<div class="flex gap-6">
			<div class="min-w-0 flex-1">
				<TimelineView events={session.events} />
			</div>
			<SessionSidebar {session} {detail} {fileStats} />
		</div>
	</div>
{/if}
