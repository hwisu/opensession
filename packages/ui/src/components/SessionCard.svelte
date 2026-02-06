<script lang="ts">
	import type { SessionListItem } from '../types';
	import { getToolConfig, formatDuration, formatTimestamp } from '../types';

	let { session }: { session: SessionListItem } = $props();

	let tool = $derived(getToolConfig(session.tool));
	let tagList = $derived(session.tags ? session.tags.split(',').filter(Boolean) : []);

	/** Strip XML-like tags (system-reminder, command-name, task-notification, etc.) */
	function stripTags(text: string): string {
		return text.replace(/<[^>]+>/g, '').replace(/\s+/g, ' ').trim();
	}

	let cleanTitle = $derived(session.title ? stripTags(session.title) : '');
	let cleanDesc = $derived(session.description ? stripTags(session.description) : '');

	let displayTitle = $derived(
		cleanTitle
			? cleanTitle
			: cleanDesc
				? cleanDesc.length > 80
					? cleanDesc.slice(0, 77) + '...'
					: cleanDesc
				: 'Untitled Session'
	);
</script>

<a
	href="/session/{session.id}"
	class="group block rounded-lg border border-border bg-bg-secondary px-4 py-3 transition-colors hover:border-border-light hover:bg-bg-tertiary"
>
	<div class="flex items-start gap-3">
		<!-- User avatar + AI tool badge -->
		<div class="relative mt-0.5 shrink-0">
			{#if session.avatar_url}
				<img
					src={session.avatar_url}
					alt={session.nickname ?? 'user'}
					class="h-8 w-8 rounded-full"
				/>
			{:else}
				<div class="flex h-8 w-8 items-center justify-center rounded-full bg-bg-hover text-xs font-bold text-text-secondary">
					{(session.nickname ?? '?')[0].toUpperCase()}
				</div>
			{/if}
			<!-- AI tool badge -->
			<div
				class="absolute -bottom-1 -right-1 flex h-[18px] w-[18px] items-center justify-center rounded text-[9px] font-bold text-white ring-2 ring-bg-secondary"
				style="background-color: {tool.color}"
			>
				{tool.icon}
			</div>
		</div>

		<div class="min-w-0 flex-1">
			<div class="flex items-start justify-between gap-2">
				<h3 class="truncate text-sm font-medium text-text-primary group-hover:text-white">
					{displayTitle}
				</h3>
				<span class="shrink-0 text-xs text-text-muted">
					{formatTimestamp(session.created_at)}
				</span>
			</div>
			<p class="mt-0.5 text-xs text-text-muted">
				{session.nickname ?? 'anonymous'} &middot; {tool.label} &middot; {session.agent_model ?? 'unknown'}
			</p>
		</div>
	</div>

	{#if cleanDesc && cleanTitle}
		<p class="mt-2 ml-11 line-clamp-1 text-xs text-text-secondary">
			{cleanDesc}
		</p>
	{/if}

	<div class="mt-2 ml-11 flex items-center gap-3 text-xs text-text-muted">
		<span>{session.message_count} msgs</span>
		<span>{session.event_count} events</span>
		{#if session.task_count > 0}
			<span>{session.task_count} tasks</span>
		{/if}
		<span>{formatDuration(session.duration_seconds)}</span>
		{#if tagList.length > 0}
			<span class="text-text-muted">&middot;</span>
			{#each tagList.slice(0, 3) as tag}
				<span class="rounded-full bg-bg-hover px-1.5 py-0 text-[10px] text-text-secondary">
					{tag}
				</span>
			{/each}
			{#if tagList.length > 3}
				<span class="text-[10px] text-text-muted">+{tagList.length - 3}</span>
			{/if}
		{/if}
	</div>
</a>
