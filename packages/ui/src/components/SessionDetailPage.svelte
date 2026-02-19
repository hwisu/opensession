<script lang="ts">
import { getSession } from '../api';
import { buildNativeFilterOptions, buildUnifiedFilterOptions, type SessionViewMode } from '../session-filters';
import type { Session, SessionDetail } from '../types';
import SessionRenderPage from './SessionRenderPage.svelte';

const { sessionId }: { sessionId: string } = $props();

let session = $state<Session | null>(null);
let detail = $state<SessionDetail | null>(null);
let loading = $state(true);
let error = $state<string | null>(null);
let viewMode = $state<SessionViewMode>('unified');
let unifiedFilters = $state(new Set<string>());
let nativeFilters = $state(new Set<string>());
let initializedForSessionId = $state<string | null>(null);

async function fetchDetail(id: string): Promise<SessionDetail> {
	const baseUrl = window.location.origin;
	const res = await fetch(`${baseUrl}/api/sessions/${encodeURIComponent(id)}`);
	if (!res.ok) throw new Error('Failed to load session detail');
	return res.json();
}

function toggleUnifiedFilter(key: string) {
	const next = new Set(unifiedFilters);
	if (next.has(key)) next.delete(key);
	else next.add(key);
	unifiedFilters = next;
}

function toggleNativeFilter(key: string) {
	const next = new Set(nativeFilters);
	if (next.has(key)) next.delete(key);
	else next.add(key);
	nativeFilters = next;
}

function initializeFilters(target: Session) {
	const allUnified = new Set(buildUnifiedFilterOptions(target.events).map((option) => option.key));
	const allNative = new Set(buildNativeFilterOptions(target.events).map((option) => option.key));
	unifiedFilters = allUnified;
	nativeFilters = allNative;
}

$effect(() => {
	loading = true;
	error = null;
	Promise.all([getSession(sessionId), fetchDetail(sessionId)])
		.then(([loadedSession, loadedDetail]) => {
			session = loadedSession;
			detail = loadedDetail;
			initializedForSessionId = null;
		})
		.catch((e) => {
			error = e instanceof Error ? e.message : 'Failed to load session';
		})
		.finally(() => {
			loading = false;
		});
});

$effect(() => {
	if (!session) return;
	if (initializedForSessionId === session.session_id) return;
	initializeFilters(session);
	initializedForSessionId = session.session_id;
});
</script>

{#if loading}
	<div class="py-16 text-center text-xs text-text-muted">Loading session...</div>
{:else if error}
	<div class="py-16 text-center">
		<p class="text-xs text-error">{error}</p>
		<a href="/" class="mt-2 inline-block text-xs text-accent hover:underline">Back to feed</a>
	</div>
{:else if session}
	<SessionRenderPage
		{session}
		{detail}
		{viewMode}
		nativeAdapter={session.agent.tool}
		{unifiedFilters}
		{nativeFilters}
		onViewModeChange={(mode) => (viewMode = mode)}
		onToggleUnifiedFilter={toggleUnifiedFilter}
		onToggleNativeFilter={toggleNativeFilter}
	/>
{/if}
