<script lang="ts">
import { prepareTimelineEvents } from '../event-helpers';
import { ApiError, getSession, getSessionDetail } from '../api';
import { buildNativeFilterOptions, toggleAllBackedFilter, type SessionViewMode } from '../session-filters';
import type { Session, SessionDetail } from '../types';
import SessionRenderPage from './SessionRenderPage.svelte';

const { sessionId }: { sessionId: string } = $props();

let session = $state<Session | null>(null);
let detail = $state<SessionDetail | null>(null);
let loading = $state(true);
let error = $state<string | null>(null);
let errorCode = $state<string | null>(null);
let viewMode = $state<SessionViewMode>('unified');
let unifiedFilters = $state(new Set<string>());
let branchFilters = $state(new Set<string>());
let nativeFilters = $state(new Set<string>());
let initializedForSessionId = $state<string | null>(null);
const ALL_FILTER_KEY = 'all';

function toggleUnifiedFilter(key: string) {
	unifiedFilters = toggleAllBackedFilter(unifiedFilters, key, ALL_FILTER_KEY);
}

function toggleNativeFilter(key: string) {
	const next = new Set(nativeFilters);
	if (next.has(key)) next.delete(key);
	else next.add(key);
	nativeFilters = next;
}

function toggleBranchFilter(key: string) {
	branchFilters = toggleAllBackedFilter(branchFilters, key, ALL_FILTER_KEY);
}

function initializeFilters(target: Session) {
	const timelineEvents = prepareTimelineEvents(target.events);
	const allUnified = new Set<string>([ALL_FILTER_KEY]);
	const allBranch = new Set<string>([ALL_FILTER_KEY]);
	const allNative = new Set(buildNativeFilterOptions(timelineEvents).map((option) => option.key));
	unifiedFilters = allUnified;
	branchFilters = allBranch;
	nativeFilters = allNative;
}

$effect(() => {
	loading = true;
	error = null;
	errorCode = null;
	Promise.all([getSession(sessionId), getSessionDetail(sessionId)])
		.then(([loadedSession, loadedDetail]) => {
			session = loadedSession;
			detail = loadedDetail;
			initializedForSessionId = null;
		})
		.catch((e) => {
			error = e instanceof Error ? e.message : 'Failed to load session';
			errorCode = e instanceof ApiError ? e.code : null;
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
		{#if errorCode === 'desktop_contract_mismatch'}
			<p class="text-xs text-error">Desktop runtime and UI bundle contract versions do not match.</p>
			<p class="mt-1 text-xs text-text-muted">Update desktop app/runtime and reopen this session.</p>
		{:else}
			<p class="text-xs text-error">{error}</p>
		{/if}
		<a href="/sessions" class="mt-2 inline-block text-xs text-accent hover:underline">Back to feed</a>
	</div>
{:else if session}
	<SessionRenderPage
		{session}
		{detail}
		{viewMode}
		nativeAdapter={session.agent.tool}
		{unifiedFilters}
		{branchFilters}
		{nativeFilters}
		onViewModeChange={(mode) => (viewMode = mode)}
		onToggleUnifiedFilter={toggleUnifiedFilter}
		onToggleBranchFilter={toggleBranchFilter}
		onToggleNativeFilter={toggleNativeFilter}
	/>
{/if}
