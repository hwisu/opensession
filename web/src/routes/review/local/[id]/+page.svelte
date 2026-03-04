<script lang="ts">
import { page } from '$app/stores';
import { untrack } from 'svelte';
import {
	buildNativeFilterOptions,
	buildUnifiedFilterOptions,
	getLocalReviewBundle,
	type LocalReviewBundle,
	type LocalReviewCommit,
	type LocalReviewReviewerDigest,
	type LocalReviewSemanticSummary,
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

type ReviewSelection = {
	commitIndex: number;
	sessionIndex: number;
};

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

const selectedCommitSummary = $derived.by((): LocalReviewSemanticSummary | null => {
	const commit = selectedCommit;
	return commit?.semantic_summary ?? null;
});

const selectedReviewerDigest = $derived.by((): LocalReviewReviewerDigest | null => {
	const commit = selectedCommit;
	return commit?.reviewer_digest ?? null;
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

function resolveSelectionFromQuery(
	loaded: LocalReviewBundle,
	sessionId: string | null,
	commitSha: string | null,
): ReviewSelection | null {
	const normalizedSession = sessionId?.trim();
	const normalizedCommit = commitSha?.trim();
	if (!normalizedSession && !normalizedCommit) return null;

	if (normalizedCommit) {
		const commitIndex = loaded.commits.findIndex((commit) => commit.sha === normalizedCommit);
		if (commitIndex >= 0) {
			const commit = loaded.commits[commitIndex];
			if (!commit) return null;
			if (normalizedSession) {
				const sessionIndex = commit.session_ids.findIndex((id) => id === normalizedSession);
				if (sessionIndex >= 0) {
					return { commitIndex, sessionIndex };
				}
			}
			return { commitIndex, sessionIndex: 0 };
		}
	}

	if (normalizedSession) {
		for (let commitIndex = 0; commitIndex < loaded.commits.length; commitIndex += 1) {
			const commit = loaded.commits[commitIndex];
			if (!commit) continue;
			const sessionIndex = commit.session_ids.findIndex((id) => id === normalizedSession);
			if (sessionIndex >= 0) {
				return { commitIndex, sessionIndex };
			}
		}
	}

	return null;
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
		const querySelection = resolveSelectionFromQuery(
			loaded,
			$page.url.searchParams.get('session'),
			$page.url.searchParams.get('commit'),
		);
		if (querySelection) {
			selectedCommitIndex = querySelection.commitIndex;
			selectedSessionIndex = querySelection.sessionIndex;
		}
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
				{#if bundle.pr.number > 0}
					PR #{bundle.pr.number} {bundle.pr.owner}/{bundle.pr.repo}
				{:else}
					Review {bundle.pr.owner}/{bundle.pr.repo}
				{/if}
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
						<div class="px-3 py-4 text-xs text-text-muted">No commits found for this review range.</div>
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
					<div class="rounded border border-border bg-bg-secondary p-3">
						<div class="border-b border-border pb-2 text-xs font-medium text-text-secondary">
							Reviewer Quick Digest
						</div>
						{#if !selectedCommit}
							<div class="px-1 py-3 text-xs text-text-muted">Select a commit.</div>
						{:else if selectedReviewerDigest}
							<div class="space-y-3 pt-3">
								<div class="grid gap-2 text-xs sm:grid-cols-3">
									<div class="rounded border border-border/70 p-2">
										<div class="text-[11px] uppercase tracking-[0.08em] text-text-muted">Q&A Pairs</div>
										<div class="mt-1 text-text-primary">{selectedReviewerDigest.qa.length}</div>
									</div>
									<div class="rounded border border-border/70 p-2">
										<div class="text-[11px] uppercase tracking-[0.08em] text-text-muted">Modified Files</div>
										<div class="mt-1 text-text-primary">{selectedReviewerDigest.modified_files.length}</div>
									</div>
									<div class="rounded border border-border/70 p-2">
										<div class="text-[11px] uppercase tracking-[0.08em] text-text-muted">Test Files</div>
										<div class="mt-1 text-text-primary">{selectedReviewerDigest.test_files.length}</div>
									</div>
								</div>
								<div class="grid gap-2 sm:grid-cols-2">
									<div class="rounded border border-border/70 p-2 text-xs">
										<div class="text-[11px] uppercase tracking-[0.08em] text-text-muted">Modified Paths</div>
										{#if selectedReviewerDigest.modified_files.length === 0}
											<div class="mt-1 text-text-muted">No file edits captured in mapped sessions.</div>
										{:else}
											<div class="mt-1 space-y-1">
												{#each selectedReviewerDigest.modified_files.slice(0, 12) as filePath}
													<div class="font-mono text-[11px] text-text-secondary">{filePath}</div>
												{/each}
												{#if selectedReviewerDigest.modified_files.length > 12}
													<div class="text-[11px] text-text-muted">
														... {selectedReviewerDigest.modified_files.length - 12} more
													</div>
												{/if}
											</div>
										{/if}
										</div>
										<div class="rounded border border-border/70 p-2 text-xs">
											<div class="text-[11px] uppercase tracking-[0.08em] text-text-muted">Interactive Q&A</div>
											{#if selectedReviewerDigest.qa.length === 0}
												<div class="mt-1 text-text-muted">No interactive Q&A content detected.</div>
											{:else}
												<div class="mt-1 space-y-2">
													{#each selectedReviewerDigest.qa.slice(0, 6) as qa}
														<div class="rounded border border-border/60 p-2">
															<div class="text-[11px] text-text-muted">Q</div>
															<div class="text-[11px] text-text-secondary">{qa.question}</div>
															{#if qa.answer}
																<div class="mt-1 text-[11px] text-text-muted">A</div>
																<div class="text-[11px] text-text-secondary">{qa.answer}</div>
															{/if}
														</div>
													{/each}
													{#if selectedReviewerDigest.qa.length > 6}
														<div class="text-[11px] text-text-muted">
															... {selectedReviewerDigest.qa.length - 6} more
														</div>
													{/if}
												</div>
											{/if}
										</div>
										<div class="rounded border border-border/70 p-2 text-xs">
											<div class="text-[11px] uppercase tracking-[0.08em] text-text-muted">Added/Updated Tests</div>
											{#if selectedReviewerDigest.test_files.length === 0}
												<div class="mt-1 text-text-muted">No test files detected.</div>
										{:else}
											<div class="mt-1 space-y-1">
												{#each selectedReviewerDigest.test_files as filePath}
													<div class="font-mono text-[11px] text-text-secondary">{filePath}</div>
												{/each}
											</div>
										{/if}
									</div>
								</div>
							</div>
						{:else}
							<div class="px-1 py-3 text-xs text-text-muted">
								No reviewer digest is available for this commit.
							</div>
						{/if}
					</div>

					<div class="rounded border border-border bg-bg-secondary p-3">
						<div class="border-b border-border pb-2 text-xs font-medium text-text-secondary">
							Commit Semantic Summary
					</div>
					{#if !selectedCommit}
						<div class="px-1 py-3 text-xs text-text-muted">Select a commit.</div>
					{:else if selectedCommitSummary}
						<div class="space-y-2 pt-3">
							<div>
								<p class="text-[11px] uppercase tracking-[0.08em] text-text-muted">Changes</p>
								<p class="mt-1 text-xs text-text-primary">{selectedCommitSummary.changes}</p>
							</div>
							<div>
								<p class="text-[11px] uppercase tracking-[0.08em] text-text-muted">Auth/Security</p>
								<p class="mt-1 text-xs text-text-primary">{selectedCommitSummary.auth_security}</p>
							</div>
							{#if selectedCommitSummary.layer_file_changes.length}
								<div>
									<p class="text-[11px] uppercase tracking-[0.08em] text-text-muted">Layer/File Changes</p>
									<div class="mt-2 space-y-2">
										{#each selectedCommitSummary.layer_file_changes as row}
											<div class="rounded border border-border/70 p-2 text-xs">
												<div class="font-medium text-text-primary">{row.layer}</div>
												<div class="mt-1 text-text-secondary">{row.summary}</div>
												{#if row.files.length}
													<div class="mt-1 text-[11px] text-text-muted">{row.files.join(', ')}</div>
												{/if}
											</div>
										{/each}
									</div>
								</div>
							{/if}
							{#if selectedCommitSummary.diff_tree.length}
								<div>
									<p class="text-[11px] uppercase tracking-[0.08em] text-text-muted">Diff Tree</p>
									<div class="mt-2 space-y-2">
										{#each selectedCommitSummary.diff_tree as layer}
											{@const layerObj = layer as {
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
													hunks?: Array<{ header?: string; lines?: string[]; omitted_lines?: number }>;
												}>;
											}}
											<details class="rounded border border-border/70 p-2" open={layerObj.file_count != null && layerObj.file_count <= 5}>
												<summary class="cursor-pointer text-xs text-text-primary">
													{layerObj.layer ?? '(layer)'} · files={layerObj.file_count ?? 0} · +{layerObj.lines_added ?? 0}/-{layerObj.lines_removed ?? 0}
												</summary>
												<div class="mt-2 space-y-2">
													{#each layerObj.files ?? [] as file}
														<details class="rounded border border-border/60 p-2" open={!file.is_large}>
															<summary class="cursor-pointer text-[11px] text-text-secondary">
																{file.path ?? '(file)'} [{file.operation ?? 'edit'}] +{file.lines_added ?? 0}/-{file.lines_removed ?? 0}
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
							{#if selectedCommitSummary.error}
								<p class="text-xs text-warning">generation note: {selectedCommitSummary.error}</p>
							{/if}
						</div>
					{:else}
						<div class="px-1 py-3 text-xs text-text-muted">No semantic summary generated for this commit.</div>
					{/if}
				</div>

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
						nativeAdapter={(selectedSession.session as Session).agent.tool}
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
