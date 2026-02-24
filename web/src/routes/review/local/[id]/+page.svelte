<script lang="ts">
import { page } from '$app/stores';
import { untrack } from 'svelte';
import {
	buildNativeFilterOptions,
	buildUnifiedFilterOptions,
	getLocalReviewBundle,
	type LocalReviewBundle,
	type LocalReviewCommit,
	type LocalReviewSession,
	type Session,
	type SessionViewMode,
} from '@opensession/ui';
import { SessionRenderPage } from '@opensession/ui/components';

type PageState = 'loading' | 'ready' | 'error';

let pageState = $state<PageState>('loading');
let errorMessage = $state<string | null>(null);
let bundle = $state<LocalReviewBundle | null>(null);
let selectedCommitIndex = $state(0);
let selectedSessionIndex = $state(0);
let viewMode = $state<SessionViewMode>('unified');
let unifiedFilters = $state(new Set<string>());
let nativeFilters = $state(new Set<string>());
let routeVersion = $state(0);
let lastObservedHref = '';

const selectedCommit = $derived.by((): LocalReviewCommit | null => {
	if (!bundle || bundle.commits.length === 0) return null;
	const idx = Math.min(selectedCommitIndex, bundle.commits.length - 1);
	return bundle.commits[idx] ?? null;
});

const selectedSession = $derived.by((): LocalReviewSession | null => {
	if (!bundle) return null;
	const commit = selectedCommit;
	if (!commit || commit.session_ids.length === 0) return null;
	const idx = Math.min(selectedSessionIndex, commit.session_ids.length - 1);
	const sessionId = commit.session_ids[idx];
	if (!sessionId) return null;

	const fromCommit = bundle.sessions.find(
		(row) => row.session_id === sessionId && row.commit_shas.includes(commit.sha),
	);
	if (fromCommit) return fromCommit;
	return bundle.sessions.find((row) => row.session_id === sessionId) ?? null;
});

function selectCommit(index: number) {
	selectedCommitIndex = index;
	selectedSessionIndex = 0;
}

function selectSession(index: number) {
	selectedSessionIndex = index;
}

function applyFiltersFromSession(session: Session | null) {
	if (!session) {
		unifiedFilters = new Set();
		nativeFilters = new Set();
		return;
	}
	unifiedFilters = new Set(buildUnifiedFilterOptions(session.events).map((option) => option.key));
	nativeFilters = new Set(buildNativeFilterOptions(session.events).map((option) => option.key));
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

function changeViewMode(mode: SessionViewMode) {
	viewMode = mode;
}

async function loadReviewBundle() {
	const activeVersion = ++routeVersion;
	pageState = 'loading';
	errorMessage = null;
	const reviewId = $page.params.id;
	if (!reviewId) {
		pageState = 'error';
		errorMessage = 'Missing review id.';
		return;
	}
	try {
		const loaded = await getLocalReviewBundle(reviewId);
		if (activeVersion !== routeVersion) return;
		bundle = loaded;
		selectedCommitIndex = 0;
		selectedSessionIndex = 0;
		applyFiltersFromSession((selectedSession?.session as Session) ?? null);
		pageState = 'ready';
	} catch (error) {
		if (activeVersion !== routeVersion) return;
		pageState = 'error';
		bundle = null;
		errorMessage =
			error instanceof Error ? error.message : 'Failed to load local review bundle.';
	}
}

$effect(() => {
	const current = selectedSession;
	applyFiltersFromSession((current?.session as Session) ?? null);
});

$effect(() => {
	const href = $page.url.href;
	if (href === lastObservedHref) return;
	lastObservedHref = href;
	untrack(() => {
		void loadReviewBundle();
	});
});
</script>

{#if pageState === 'loading'}
	<div class="py-16 text-center text-xs text-text-muted">Loading local review bundle...</div>
{:else if pageState === 'error'}
	<div class="mx-auto max-w-3xl border border-error/30 bg-error/10 px-4 py-3 text-sm text-error">
		{errorMessage ?? 'Failed to load review bundle.'}
	</div>
{:else if bundle}
	<div class="space-y-3">
		<div class="rounded border border-border bg-bg-secondary px-3 py-2 text-xs text-text-secondary">
			<div class="font-medium text-text-primary">
				PR #{bundle.pr.number} {bundle.pr.owner}/{bundle.pr.repo}
			</div>
			<div class="mt-1 break-all">
				base <span class="font-mono">{bundle.pr.base_sha}</span> -> head
				<span class="font-mono">{bundle.pr.head_sha}</span>
			</div>
		</div>

		<div class="grid gap-3 lg:grid-cols-[320px_minmax(0,1fr)]">
			<div class="rounded border border-border bg-bg-secondary">
				<div class="border-b border-border px-3 py-2 text-xs font-medium text-text-secondary">
					Commit Groups ({bundle.commits.length})
				</div>
				<div class="max-h-[72vh] overflow-auto">
					{#if bundle.commits.length === 0}
						<div class="px-3 py-4 text-xs text-text-muted">No commits found for this PR range.</div>
					{:else}
						{#each bundle.commits as commit, index}
							<button
								type="button"
								class="w-full border-b border-border/60 px-3 py-2 text-left transition-colors hover:bg-bg-tertiary/40"
								class:bg-bg-tertiary={index === selectedCommitIndex}
								onclick={() => selectCommit(index)}
							>
								<div class="font-mono text-[11px] text-text-muted">{commit.sha.slice(0, 7)}</div>
								<div class="mt-0.5 truncate text-xs text-text-primary">{commit.title}</div>
								<div class="mt-1 text-[11px] text-text-muted">
									{commit.session_ids.length} sessions · {commit.author_name}
								</div>
							</button>
						{/each}
					{/if}
				</div>
			</div>

			<div class="space-y-3">
				<div class="rounded border border-border bg-bg-secondary">
					<div class="border-b border-border px-3 py-2 text-xs font-medium text-text-secondary">
						Sessions
					</div>
					{#if !selectedCommit}
						<div class="px-3 py-4 text-xs text-text-muted">Select a commit to inspect sessions.</div>
					{:else if selectedCommit.session_ids.length === 0}
						<div class="px-3 py-4 text-xs text-text-muted">
							No mapped sessions for commit {selectedCommit.sha.slice(0, 7)}.
						</div>
					{:else}
						<div class="flex flex-wrap gap-2 px-3 py-3">
							{#each selectedCommit.session_ids as sessionId, index}
								<button
									type="button"
									class="rounded border border-border px-2 py-1 text-xs transition-colors hover:bg-bg-tertiary/40"
									class:bg-bg-tertiary={index === selectedSessionIndex}
									onclick={() => selectSession(index)}
								>
									{sessionId}
								</button>
							{/each}
						</div>
					{/if}
				</div>

				{#if selectedSession}
					<SessionRenderPage
						session={selectedSession.session as Session}
						viewMode={viewMode}
						unifiedFilters={unifiedFilters}
						nativeFilters={nativeFilters}
						onViewModeChange={changeViewMode}
						onToggleUnifiedFilter={toggleUnifiedFilter}
						onToggleNativeFilter={toggleNativeFilter}
					/>
				{:else}
					<div class="rounded border border-border bg-bg-secondary px-4 py-12 text-center text-xs text-text-muted">
						No session selected.
					</div>
				{/if}
			</div>
		</div>
	</div>
{/if}
