<script lang="ts">
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

{#snippet statRow(label: string, content: string)}
	<div class="flex items-center gap-2">
		<span class="shrink-0 text-text-muted">{label}</span>
		<span class="text-text-secondary">{content}</span>
	</div>
{/snippet}

<aside
	class="hidden w-64 shrink-0 overflow-y-auto border-l border-border bg-[linear-gradient(180deg,rgba(24,33,50,0.45),rgba(14,19,29,0.75))] px-3 py-3 lg:block"
>
	<div class="space-y-3">
		<!-- User -->
		{#if detail?.nickname}
			<div class="flex items-center gap-2 rounded border border-border/70 bg-bg-primary/60 px-2 py-1.5 text-sm">
				<span class="font-medium text-text-primary">{detail.nickname}</span>
			</div>
		{/if}

		<h3 class="text-xs font-semibold uppercase tracking-wider text-text-muted">Session</h3>

		<div class="space-y-2 rounded border border-border/70 bg-bg-primary/55 p-2 text-xs">
			<!-- Date -->
			<div>
				<div class="text-text-secondary">{formatTimestamp(session.context.created_at)}</div>
				<div class="text-text-muted">{formatFullDate(session.context.created_at)}</div>
			</div>

			{@render statRow('Model:', session.agent.model)}

			<!-- Tool (with optional version) -->
			<div class="flex items-center gap-2">
				<span class="text-text-muted">Tool:</span>
				<span class="text-text-secondary">{tool.label}</span>
				{#if session.agent.tool_version}
					<span class="text-text-muted">v{session.agent.tool_version}</span>
				{/if}
			</div>

			{@render statRow('Provider:', session.agent.provider)}

			<hr class="border-border/60" />

			{@render statRow('Messages:', `${session.stats.message_count}`)}
			{@render statRow('Tools:', `${session.stats.tool_call_count}`)}
			{@render statRow('Duration:', formatDuration(session.stats.duration_seconds))}

			{#if fileStats.filesChanged > 0}
				{@render statRow('Files:', `${fileStats.filesChanged} changed`)}
			{/if}

			{#if fileStats.linesAdded > 0 || fileStats.linesRemoved > 0}
				<div class="flex items-center gap-2">
					<span class="text-text-muted">Lines:</span>
					<span>
						<span class="text-success">+{fileStats.linesAdded}</span>
						<span class="text-error">-{fileStats.linesRemoved}</span>
					</span>
				</div>
			{/if}

			{#if session.stats.task_count > 0}
				{@render statRow('Tasks:', `${session.stats.task_count}`)}
			{/if}
		</div>

		<!-- Tags -->
		{#if session.context.tags.length > 0}
			<div class="flex flex-wrap gap-1 rounded border border-border/70 bg-bg-primary/55 p-2 text-xs">
				{#each session.context.tags as tag}
					<span class="rounded border border-border/70 bg-bg-secondary px-1.5 py-0.5 text-text-secondary">
						#{tag}
					</span>
				{/each}
			</div>
		{/if}
	</div>
</aside>
