<script lang="ts">
import { getSession } from '../api';
import { SCROLL_STEP_PX } from '../constants';
import type { Session, SessionDetail } from '../types';
import { formatDuration, formatTimestamp, getToolConfig } from '../types';
import { computeFileStats, getDisplayTitle } from '../utils';
import SessionSidebar from './SessionSidebar.svelte';
import TimelineView from './TimelineView.svelte';

const { sessionId }: { sessionId: string } = $props();

let session = $state<Session | null>(null);
let detail = $state<SessionDetail | null>(null);
let loading = $state(true);
let error = $state<string | null>(null);

const tool = $derived(session ? getToolConfig(session.agent.tool) : null);
const displayTitle = $derived(session ? getDisplayTitle(session) : 'Session');
const fileStats = $derived(
	session ? computeFileStats(session.events) : { filesChanged: 0, linesAdded: 0, linesRemoved: 0 },
);

let timelineEl: HTMLDivElement | undefined = $state();

function handleKeydown(e: KeyboardEvent) {
	if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
	if (!timelineEl) return;
	if (e.key === 'j') {
		e.preventDefault();
		timelineEl.scrollBy({ top: SCROLL_STEP_PX, behavior: 'smooth' });
	} else if (e.key === 'k') {
		e.preventDefault();
		timelineEl.scrollBy({ top: -SCROLL_STEP_PX, behavior: 'smooth' });
	}
}

async function fetchDetail(id: string): Promise<SessionDetail> {
	const baseUrl = window.location.origin;
	const res = await fetch(`${baseUrl}/api/sessions/${encodeURIComponent(id)}`);
	if (!res.ok) throw new Error('Failed to load session detail');
	return res.json();
}

$effect(() => {
	loading = true;
	error = null;
	Promise.all([getSession(sessionId), fetchDetail(sessionId)])
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

<svelte:window onkeydown={handleKeydown} />

<svelte:head>
	<title>{displayTitle} - opensession.io</title>
</svelte:head>

{#if loading}
	<div class="py-16 text-center text-xs text-text-muted">Loading session...</div>
{:else if error}
	<div class="py-16 text-center">
		<p class="text-xs text-error">{error}</p>
		<a href="/" class="mt-2 inline-block text-xs text-accent hover:underline">Back to feed</a>
	</div>
{:else if session && tool}
	<div class="flex h-full flex-col">
		<div class="shrink-0 border-b border-border px-3 py-2">
			<h1 class="truncate text-lg font-bold text-text-primary">
				{displayTitle}
			</h1>
			<div class="mt-1 flex flex-wrap items-center gap-2 text-xs text-text-muted">
				<span class="tui-badge tui-badge-tool" style="background-color: {tool.color}">{tool.icon}</span>
				<span>{tool.label}</span>
				<span>&middot;</span>
				<span class="text-text-secondary">{session.agent.model}</span>
				<span>&middot;</span>
				<span>{formatDuration(session.stats.duration_seconds)}</span>
				<span>&middot;</span>
				<span>{session.stats.message_count} msgs</span>
				{#if fileStats.filesChanged > 0}
					<span>&middot;</span>
					<span>{fileStats.filesChanged} files
						(<span class="text-success">+{fileStats.linesAdded}</span>
						<span class="text-error">-{fileStats.linesRemoved}</span>)
					</span>
				{/if}
				<span>&middot;</span>
				<span>{formatTimestamp(session.context.created_at)}</span>
			</div>
		</div>

		<div class="flex min-h-0 flex-1 overflow-hidden">
			<div bind:this={timelineEl} class="flex-1 overflow-y-auto px-3 py-2">
				<TimelineView events={session.events} />
			</div>
			<SessionSidebar {session} {detail} {fileStats} />
		</div>
	</div>
{/if}
