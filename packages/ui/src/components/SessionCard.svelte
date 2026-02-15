<script lang="ts">
import { truncate } from '../event-helpers';
import { getSessionActorLabel, getSessionModelLabel } from '../session-presentation';
import type { SessionListItem } from '../types';
import { formatDuration, formatTimestamp, getToolConfig } from '../types';
import { stripTags } from '../utils';

const {
	session,
	selected = false,
}: {
	session: SessionListItem;
	selected?: boolean;
} = $props();

const tool = $derived(getToolConfig(session.tool));

const cleanTitle = $derived(session.title ? stripTags(session.title) : '');
const cleanDesc = $derived(session.description ? stripTags(session.description) : '');

const displayTitle = $derived(
	cleanTitle ? cleanTitle : cleanDesc ? truncate(cleanDesc) : 'Untitled Session',
);
const actorLabel = $derived(getSessionActorLabel(session));
const modelLabel = $derived(getSessionModelLabel(session));
</script>

<a
	href="/session/{session.id}"
	class="group flex items-center gap-3 px-3 py-1.5 text-xs transition-colors hover:bg-bg-hover"
	class:bg-bg-hover={selected}
>
	<!-- Cursor -->
	<span class="w-2 shrink-0 text-accent">{selected ? '>' : ''}</span>

	<!-- Tool badge (2-char) -->
	<span
		class="tui-badge shrink-0"
		class:tui-badge-tool={true}
		style="background-color: {tool.color}"
	>
		{tool.icon}
	</span>

	<!-- Title (truncate) -->
	<span class="min-w-0 flex-1 truncate text-text-primary group-hover:text-accent">
		{displayTitle}
	</span>

	{#if actorLabel}
		<span class="hidden shrink-0 text-[11px] text-accent lg:inline">
			{actorLabel}
		</span>
	{/if}

	<!-- Date -->
	<span class="hidden shrink-0 text-text-muted sm:inline">
		{formatTimestamp(session.created_at)}
	</span>

	<!-- Model (colored) -->
	<span class="hidden shrink-0 text-text-secondary md:inline">
		{modelLabel}
	</span>

	<!-- Stats -->
	<span class="hidden shrink-0 text-text-muted lg:inline">
		{session.message_count} msgs
	</span>
	<span class="hidden shrink-0 text-text-muted lg:inline">
		{session.event_count} ev
	</span>
	<span class="shrink-0 text-text-muted">
		{formatDuration(session.duration_seconds)}
	</span>
</a>
