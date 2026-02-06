<script lang="ts">
	import { page } from '$app/stores';
	import { getSession } from '$lib/api';
	import { getToolConfig, formatDuration, formatTimestamp } from '$lib/types';
	import type { SessionDetail } from '$lib/types';
	import TimelineView from '$lib/components/TimelineView.svelte';
	import type { Session } from '$lib/types';

	let session = $state<Session | null>(null);
	let detail = $state<SessionDetail | null>(null);
	let loading = $state(true);
	let error = $state<string | null>(null);

	let tool = $derived(session ? getToolConfig(session.agent.tool) : null);

	/** Strip XML-like tags */
	function stripTags(text: string): string {
		return text.replace(/<[^>]+>/g, '').replace(/\s+/g, ' ').trim();
	}

	// Extract title from first user message if session title is empty
	let displayTitle = $derived.by(() => {
		if (!session) return 'Session';
		if (session.context.title) {
			const clean = stripTags(session.context.title);
			if (clean) return clean;
		}
		// Fallback: first UserMessage text
		for (const ev of session.events) {
			if (ev.event_type.type === 'UserMessage') {
				for (const block of ev.content.blocks) {
					if (block.type === 'Text' && block.text.trim()) {
						const text = stripTags(block.text.trim());
						if (!text) continue;
						return text.length > 80 ? text.slice(0, 77) + '...' : text;
					}
				}
			}
		}
		return 'Untitled Session';
	});

	// Compute file change stats from events
	let fileStats = $derived.by(() => {
		if (!session) return { filesChanged: 0, linesAdded: 0, linesRemoved: 0 };
		const files = new Set<string>();
		let added = 0;
		let removed = 0;
		for (const ev of session.events) {
			const t = ev.event_type;
			if (t.type === 'FileEdit' || t.type === 'FileCreate') {
				files.add(t.data.path);
				// Count diff lines if available
				if (t.type === 'FileEdit' && t.data.diff) {
					for (const line of t.data.diff.split('\n')) {
						if (line.startsWith('+') && !line.startsWith('+++')) added++;
						if (line.startsWith('-') && !line.startsWith('---')) removed++;
					}
				}
			} else if (t.type === 'FileDelete') {
				files.add(t.data.path);
			}
		}
		return { filesChanged: files.size, linesAdded: added, linesRemoved: removed };
	});

	// Format full date
	function formatFullDate(ts: string): string {
		const date = new Date(ts);
		return date.toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
	}

	async function fetchDetail(id: string): Promise<SessionDetail> {
		const baseUrl = window.location.origin;
		const res = await fetch(`${baseUrl}/api/sessions/${encodeURIComponent(id)}`);
		if (!res.ok) throw new Error('Failed to load session detail');
		return res.json();
	}

	$effect(() => {
		const id = $page.params.id;
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
	<title>{displayTitle} - opensession.io</title>
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
			<h1 class="text-2xl font-bold text-white leading-tight">
				{displayTitle}
			</h1>
			<div class="mt-2 flex items-center gap-3">
				<!-- User avatar + AI tool badge -->
				<div class="relative shrink-0">
					{#if detail?.avatar_url}
						<img
							src={detail.avatar_url}
							alt={detail.nickname ?? 'user'}
							class="h-8 w-8 rounded-full"
						/>
					{:else}
						<div class="flex h-8 w-8 items-center justify-center rounded-full bg-bg-hover text-xs font-bold text-text-secondary">
							{(detail?.nickname ?? '?')[0].toUpperCase()}
						</div>
					{/if}
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

		<!-- Mobile: inline metadata (shown below lg breakpoint) -->
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

		<!-- Two-column layout: Main + Sidebar -->
		<div class="flex gap-6">
			<!-- Main content -->
			<div class="min-w-0 flex-1">
				<TimelineView events={session.events} />
			</div>

			<!-- Right Sidebar -->
			<aside class="hidden w-64 shrink-0 lg:block">
				<div class="sticky top-4 space-y-4">
					<div class="rounded-lg border border-border bg-bg-secondary p-4">
						<!-- User profile -->
						{#if detail?.avatar_url || detail?.nickname}
							<div class="mb-3 flex items-center gap-2">
								{#if detail.avatar_url}
									<img src={detail.avatar_url} alt={detail.nickname ?? ''} class="h-6 w-6 rounded-full" />
								{/if}
								<span class="text-sm font-medium text-text-primary">{detail.nickname ?? 'anonymous'}</span>
							</div>
							<hr class="mb-3 border-border" />
						{/if}

						<h3 class="mb-3 text-xs font-semibold uppercase tracking-wider text-text-muted">Session</h3>

						<div class="space-y-3 text-sm">
							<!-- Date -->
							<div class="flex items-start gap-2">
								<span class="mt-0.5 shrink-0 text-text-muted">&#x1f4c5;</span>
								<div>
									<div class="text-text-secondary">{formatTimestamp(session.context.created_at)}</div>
									<div class="text-xs text-text-muted">{formatFullDate(session.context.created_at)}</div>
								</div>
							</div>

							<!-- Model -->
							<div class="flex items-center gap-2">
								<span class="shrink-0 text-text-muted">&#x2728;</span>
								<span class="truncate text-text-secondary">{session.agent.model}</span>
							</div>

							<!-- Tool -->
							<div class="flex items-center gap-2">
								<span class="shrink-0 text-text-muted">&#x2699;</span>
								<span class="text-text-secondary">{tool.label}</span>
								{#if session.agent.tool_version}
									<span class="text-xs text-text-muted">v{session.agent.tool_version}</span>
								{/if}
							</div>

							<!-- Provider -->
							<div class="flex items-center gap-2">
								<span class="shrink-0 text-text-muted">&#x2601;</span>
								<span class="text-text-secondary">{session.agent.provider}</span>
							</div>

							<!-- Divider -->
							<hr class="border-border" />

							<!-- Prompt count -->
							<div class="flex items-center gap-2">
								<span class="shrink-0 text-text-muted">&#x1f4ac;</span>
								<span class="text-text-secondary">{session.stats.message_count} messages</span>
							</div>

							<!-- Tool calls -->
							<div class="flex items-center gap-2">
								<span class="shrink-0 text-text-muted">&#x1f527;</span>
								<span class="text-text-secondary">{session.stats.tool_call_count} tool calls</span>
							</div>

							<!-- Duration -->
							<div class="flex items-center gap-2">
								<span class="shrink-0 text-text-muted">&#x23f1;</span>
								<span class="text-text-secondary">{formatDuration(session.stats.duration_seconds)}</span>
							</div>

							<!-- Files changed -->
							{#if fileStats.filesChanged > 0}
								<div class="flex items-center gap-2">
									<span class="shrink-0 text-text-muted">&#x1f4c4;</span>
									<span class="text-text-secondary">{fileStats.filesChanged} files changed</span>
								</div>
							{/if}

							<!-- Lines changed -->
							{#if fileStats.linesAdded > 0 || fileStats.linesRemoved > 0}
								<div class="flex items-center gap-2">
									<span class="shrink-0 text-text-muted">&#x2194;</span>
									<span>
										<span class="text-green-400">+{fileStats.linesAdded}</span>
										<span class="text-red-400">-{fileStats.linesRemoved}</span>
										<span class="text-xs text-text-muted">lines</span>
									</span>
								</div>
							{/if}

							<!-- Tasks -->
							{#if session.stats.task_count > 0}
								<div class="flex items-center gap-2">
									<span class="shrink-0 text-text-muted">&#x1f9e9;</span>
									<span class="text-text-secondary">{session.stats.task_count} tasks</span>
								</div>
							{/if}
						</div>

						<!-- Tags -->
						{#if session.context.tags.length > 0}
							<hr class="my-3 border-border" />
							<div class="flex flex-wrap gap-1">
								{#each session.context.tags as tag}
									<span class="rounded-full bg-bg-hover px-2 py-0.5 text-xs text-text-secondary">
										{tag}
									</span>
								{/each}
							</div>
						{/if}
					</div>
				</div>
			</aside>
		</div>
	</div>
{/if}
