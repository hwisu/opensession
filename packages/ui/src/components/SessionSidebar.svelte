<script lang="ts">
import {
	botIcon,
	calendarIcon,
	clockIcon,
	fileEditIcon,
	fileIcon,
	globeIcon,
	lightningIcon,
	listIcon,
	tagIcon,
	taskEndIcon,
	terminalIcon,
	userIcon,
} from './icons';
import type { Session, SessionDetail } from '../types';
import { formatDuration, formatTimestamp, getToolConfig } from '../types';
import type { FileStats } from '../utils';
import { formatFullDate } from '../utils';

const {
	session,
	detail = null,
	fileStats,
}: {
	session: Session;
	detail?: SessionDetail | null;
	fileStats: FileStats;
} = $props();

const tool = $derived(getToolConfig(session.agent.tool));
</script>

{#snippet glyph(icon: string)}
	<span class="inline-flex h-4 w-4 shrink-0 items-center justify-center text-text-muted [&>svg]:h-3.5 [&>svg]:w-3.5">
		{@html icon}
	</span>
{/snippet}

{#snippet statRow(icon: string, label: string, content: string)}
	<div class="flex items-center gap-2">
		{@render glyph(icon)}
		<span class="shrink-0 text-text-muted">{label}</span>
		<span class="text-text-secondary">{content}</span>
	</div>
{/snippet}

<aside
	data-testid="session-detail-sidebar"
	class="session-sidebar hidden w-64 shrink-0 overflow-y-auto border-l border-border px-3 py-3 lg:block"
>
	<div class="space-y-3">
		<!-- User -->
		{#if detail?.nickname}
			<div class="flex items-center gap-2 rounded border border-border/70 bg-bg-primary/60 px-2 py-1.5 text-sm">
				{@render glyph(userIcon)}
				<span class="font-medium text-text-primary">{detail.nickname}</span>
			</div>
		{/if}

		<h3 class="text-xs font-semibold uppercase tracking-wider text-text-muted">Session</h3>

		<div class="space-y-2 rounded border border-border/70 bg-bg-primary/55 p-2 text-xs">
			<!-- Date -->
			<div class="space-y-1">
				<div class="flex items-center gap-2">
					{@render glyph(calendarIcon)}
					<div class="text-text-secondary">{formatTimestamp(session.context.created_at)}</div>
				</div>
				<div class="flex items-center gap-2">
					{@render glyph(clockIcon)}
					<div class="text-text-muted">{formatFullDate(session.context.created_at)}</div>
				</div>
			</div>

			{@render statRow(botIcon, 'Model:', session.agent.model)}

			<!-- Tool (with optional version) -->
			<div class="flex items-center gap-2">
				{@render glyph(lightningIcon)}
				<span class="text-text-muted">Tool:</span>
				<span class="text-text-secondary">{tool.label}</span>
				{#if session.agent.tool_version}
					<span class="text-text-muted">v{session.agent.tool_version}</span>
				{/if}
			</div>

			{@render statRow(globeIcon, 'Provider:', session.agent.provider)}

			<hr class="border-border/60" />

			{@render statRow(listIcon, 'Messages:', `${session.stats.message_count}`)}
			{@render statRow(terminalIcon, 'Tools:', `${session.stats.tool_call_count}`)}
			{@render statRow(clockIcon, 'Duration:', formatDuration(session.stats.duration_seconds))}

			{#if fileStats.filesChanged > 0}
				{@render statRow(fileIcon, 'Files:', `${fileStats.filesChanged} changed`)}
			{/if}

			{#if fileStats.linesAdded > 0 || fileStats.linesRemoved > 0}
				<div class="flex items-center gap-2">
					{@render glyph(fileEditIcon)}
					<span class="text-text-muted">Lines:</span>
					<span>
						<span class="text-success">+{fileStats.linesAdded}</span>
						<span class="text-error">-{fileStats.linesRemoved}</span>
					</span>
				</div>
			{/if}

			{#if session.stats.task_count > 0}
				{@render statRow(taskEndIcon, 'Tasks:', `${session.stats.task_count}`)}
			{/if}
		</div>

		<!-- Tags -->
		{#if session.context.tags.length > 0}
			<div class="flex flex-wrap gap-1 rounded border border-border/70 bg-bg-primary/55 p-2 text-xs">
				<span class="mr-1 inline-flex items-center text-text-muted">{@html tagIcon}</span>
				{#each session.context.tags as tag}
					<span class="rounded border border-border/70 bg-bg-secondary px-1.5 py-0.5 text-text-secondary">
						#{tag}
					</span>
				{/each}
			</div>
		{/if}
	</div>
</aside>

<style>
	.session-sidebar {
		background: linear-gradient(
			180deg,
			color-mix(in oklab, var(--color-bg-secondary) 82%, transparent),
			color-mix(in oklab, var(--color-bg-primary) 90%, transparent)
		);
	}
</style>
