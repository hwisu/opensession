<script lang="ts">
import { prepareTimelineEvents } from '../event-helpers';
import {
	ApiError,
	getSession,
	getSessionDetail,
	getSessionSemanticSummary,
	regenerateSessionSemanticSummary,
} from '../api';
import {
	buildNativeFilterOptions,
	toggleAllBackedFilter,
	type SessionViewMode,
} from '../session-filters';
import type { DesktopSessionSummaryResponse, Session, SessionDetail } from '../types';
import SessionRenderPage from './SessionRenderPage.svelte';

const { sessionId }: { sessionId: string } = $props();

let session = $state<Session | null>(null);
let detail = $state<SessionDetail | null>(null);
let loading = $state(true);
let error = $state<string | null>(null);
let errorCode = $state<string | null>(null);
let summaryLoading = $state(false);
let summaryRegenerating = $state(false);
let summaryError = $state<string | null>(null);
let semanticSummary = $state<DesktopSessionSummaryResponse | null>(null);
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

const semanticPayload = $derived.by(() => {
	const payload = semanticSummary?.summary;
	if (!payload || typeof payload !== 'object') return null;
	return payload as {
		changes?: string;
		auth_security?: string;
		layer_file_changes?: Array<{ layer?: string; summary?: string; files?: string[] }>;
	};
});

const semanticDiffTree = $derived.by(() => {
	return Array.isArray(semanticSummary?.diff_tree)
		? (semanticSummary?.diff_tree as Array<{
				layer?: string;
				file_count?: number;
				lines_added?: number;
				lines_removed?: number;
				files?: Array<{
					path?: string;
					operation?: string;
					lines_added?: number;
					lines_removed?: number;
					is_large?: boolean;
					hunks?: Array<{
						header?: string;
						lines?: string[];
						lines_added?: number;
						lines_removed?: number;
						omitted_lines?: number;
					}>;
				}>;
			}>)
		: [];
});

async function regenerateSummary() {
	summaryRegenerating = true;
	summaryError = null;
	try {
		semanticSummary = await regenerateSessionSemanticSummary(sessionId);
	} catch (e) {
		summaryError = e instanceof Error ? e.message : 'Failed to regenerate summary';
	} finally {
		summaryRegenerating = false;
	}
}

$effect(() => {
	loading = true;
	error = null;
	errorCode = null;
	summaryLoading = true;
	summaryError = null;
	semanticSummary = null;
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

	getSessionSemanticSummary(sessionId)
		.then((summary) => {
			semanticSummary = summary;
		})
		.catch((e) => {
			summaryError = e instanceof Error ? e.message : 'Failed to load semantic summary';
		})
		.finally(() => {
			summaryLoading = false;
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
	<section class="mb-3 space-y-3 rounded border border-border bg-bg-secondary p-3" data-testid="semantic-summary-card">
		<div class="flex flex-wrap items-center justify-between gap-3">
			<div>
				<p class="text-[11px] uppercase tracking-[0.08em] text-text-muted">Semantic Summary</p>
				{#if semanticSummary?.source_kind}
					<p class="mt-1 text-xs text-text-secondary">
						source={semanticSummary.source_kind}
						{#if semanticSummary.generation_kind}
							· mode={semanticSummary.generation_kind}
						{/if}
					</p>
				{/if}
			</div>
			<button
				type="button"
				class="border border-border px-2 py-1 text-xs text-text-secondary hover:text-text-primary disabled:opacity-60"
				onclick={regenerateSummary}
				disabled={summaryRegenerating}
			>
				{summaryRegenerating ? 'Regenerating…' : 'Regenerate'}
			</button>
		</div>

		{#if summaryLoading}
			<p class="text-xs text-text-muted">Loading semantic summary...</p>
		{:else if summaryError}
			<p class="text-xs text-error">{summaryError}</p>
		{:else if semanticPayload}
			<div class="grid gap-3 md:grid-cols-2">
				<div>
					<p class="text-[11px] uppercase tracking-[0.08em] text-text-muted">Changes</p>
					<p class="mt-1 text-xs text-text-primary">{semanticPayload.changes ?? '(none)'}</p>
				</div>
				<div>
					<p class="text-[11px] uppercase tracking-[0.08em] text-text-muted">Auth/Security</p>
					<p class="mt-1 text-xs text-text-primary">{semanticPayload.auth_security ?? '(none)'}</p>
				</div>
			</div>
			<div>
				<p class="text-[11px] uppercase tracking-[0.08em] text-text-muted">Layer/File Changes</p>
				{#if semanticPayload.layer_file_changes?.length}
					<div class="mt-2 space-y-2">
						{#each semanticPayload.layer_file_changes as item}
							<div class="rounded border border-border/70 p-2">
								<div class="text-xs font-medium text-text-primary">{item.layer ?? '(unknown layer)'}</div>
								<div class="mt-1 text-xs text-text-secondary">{item.summary ?? ''}</div>
								{#if item.files?.length}
									<div class="mt-1 text-[11px] text-text-muted">{item.files.join(', ')}</div>
								{/if}
							</div>
						{/each}
					</div>
				{:else}
					<p class="mt-1 text-xs text-text-muted">No layer/file entries.</p>
				{/if}
			</div>
			{#if semanticDiffTree.length}
				<div>
					<p class="text-[11px] uppercase tracking-[0.08em] text-text-muted">Diff Tree</p>
					<div class="mt-2 space-y-2">
						{#each semanticDiffTree as layer}
							<details class="rounded border border-border/70 p-2" open={layer.file_count != null && layer.file_count <= 5}>
								<summary class="cursor-pointer text-xs text-text-primary">
									{layer.layer ?? '(layer)'} · files={layer.file_count ?? 0} · +{layer.lines_added ?? 0}/-{layer.lines_removed ?? 0}
								</summary>
								<div class="mt-2 space-y-2">
									{#each layer.files ?? [] as file}
										<details class="rounded border border-border/60 p-2" open={!file.is_large}>
											<summary class="cursor-pointer text-[11px] text-text-secondary">
												{file.path ?? '(file)'} [{file.operation ?? 'edit'}] +{file.lines_added ?? 0}/-{file.lines_removed ?? 0}
												{#if file.is_large}
													· large
												{/if}
											</summary>
											{#if file.hunks?.length}
												<div class="mt-2 space-y-1">
													{#each file.hunks as hunk}
														<div class="rounded border border-border/50 bg-bg-primary p-2">
															<div class="text-[11px] font-mono text-text-muted">{hunk.header ?? '(hunk)'}</div>
															{#if hunk.lines?.length}
																<pre class="mt-1 overflow-x-auto whitespace-pre-wrap text-[11px] text-text-secondary">{hunk.lines.join('\n')}</pre>
															{/if}
															{#if (hunk.omitted_lines ?? 0) > 0}
																<div class="mt-1 text-[11px] text-text-muted">… {hunk.omitted_lines} lines omitted</div>
															{/if}
														</div>
													{/each}
												</div>
											{/if}
										</details>
									{/each}
								</div>
							</details>
						{/each}
					</div>
				</div>
			{/if}
			{#if semanticSummary?.error}
				<p class="text-xs text-warning">generation note: {semanticSummary.error}</p>
			{/if}
		{:else}
			<p class="text-xs text-text-muted">No semantic summary generated yet.</p>
		{/if}
	</section>

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
