<script lang="ts">
import { page } from '$app/stores';
import { untrack } from 'svelte';
import {
	buildNativeFilterOptions,
	buildUnifiedFilterOptions,
	getJobReviewBundle,
	type JobReviewBundle,
	type JobReviewRun,
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
let bundle = $state<JobReviewBundle | null>(null);
let selectedRunIndex = $state(0);
let selectedSessionIndex = $state(0);
let viewMode = $state<SessionViewMode>('unified');
let unifiedFilters = $state(new Set<string>());
let nativeFilters = $state(new Set<string>());
let routeVersion = $state(0);
let lastObservedHref = '';

type ReviewSelection = {
	runIndex: number;
	sessionIndex: number;
};

const reviewKind = $derived.by(() => {
	const kind = $page.url.searchParams.get('kind')?.trim().toLowerCase();
	return kind === 'done' ? 'done' : 'todo';
});

const selectedRun = $derived.by((): JobReviewRun | null => {
	if (!bundle || bundle.runs.length === 0) return null;
	const idx = Math.min(selectedRunIndex, bundle.runs.length - 1);
	return bundle.runs[idx] ?? null;
});

const selectedSession = $derived.by((): LocalReviewSession | null => {
	const run = selectedRun;
	if (!run || run.sessions.length === 0) return null;
	const idx = Math.min(selectedSessionIndex, run.sessions.length - 1);
	return run.sessions[idx] ?? null;
});

const selectedReviewerDigest = $derived.by((): LocalReviewReviewerDigest | null => {
	return bundle?.review_digest ?? null;
});

const selectedSemanticSummary = $derived.by((): LocalReviewSemanticSummary | null => {
	return bundle?.semantic_summary ?? null;
});

function selectRun(index: number) {
	selectedRunIndex = index;
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
	loaded: JobReviewBundle,
	sessionId: string | null,
	runId: string | null,
): ReviewSelection | null {
	const normalizedSession = sessionId?.trim();
	const normalizedRun = runId?.trim();
	if (!normalizedSession && !normalizedRun) return null;

	if (normalizedRun) {
		const runIndex = loaded.runs.findIndex((run) => run.run_id === normalizedRun);
		if (runIndex >= 0) {
			const run = loaded.runs[runIndex];
			if (normalizedSession) {
				const sessionIndex = run.sessions.findIndex((row) => row.session_id === normalizedSession);
				if (sessionIndex >= 0) {
					return { runIndex, sessionIndex };
				}
			}
			return { runIndex, sessionIndex: 0 };
		}
	}

	if (normalizedSession) {
		for (let runIndex = 0; runIndex < loaded.runs.length; runIndex += 1) {
			const run = loaded.runs[runIndex];
			const sessionIndex = run.sessions.findIndex((row) => row.session_id === normalizedSession);
			if (sessionIndex >= 0) {
				return { runIndex, sessionIndex };
			}
		}
	}

	return null;
}

async function loadJobReviewBundle() {
	const activeVersion = ++routeVersion;
	pageState = 'loading';
	errorMessage = null;
	const jobId = $page.params.job_id;
	if (!jobId) {
		pageState = 'error';
		errorMessage = 'Missing job id.';
		return;
	}

	try {
		const loaded = await getJobReviewBundle(
			jobId,
			reviewKind,
			$page.url.searchParams.get('run_id'),
		);
		if (activeVersion !== routeVersion) return;
		bundle = loaded;
		selectedRunIndex = 0;
		selectedSessionIndex = 0;
		const querySelection = resolveSelectionFromQuery(
			loaded,
			$page.url.searchParams.get('session'),
			$page.url.searchParams.get('run_id'),
		);
		if (querySelection) {
			selectedRunIndex = querySelection.runIndex;
			selectedSessionIndex = querySelection.sessionIndex;
		}
		applyFiltersFromSession((selectedSession?.session as Session) ?? null);
		pageState = 'ready';
	} catch (error) {
		if (activeVersion !== routeVersion) return;
		pageState = 'error';
		bundle = null;
		errorMessage = error instanceof Error ? error.message : 'Failed to load job review bundle.';
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
		void loadJobReviewBundle();
	});
});
</script>

{#if pageState === 'loading'}
	<div class="py-16 text-center text-xs text-text-muted">Loading job review bundle...</div>
{:else if pageState === 'error'}
	<div class="mx-auto max-w-3xl border border-error/30 bg-error/10 px-4 py-3 text-sm text-error">
		{errorMessage ?? 'Failed to load job review bundle.'}
	</div>
{:else if bundle}
	<div class="space-y-3">
		<div class="rounded border border-border bg-bg-secondary px-3 py-2 text-xs text-text-secondary">
			<div class="font-medium text-text-primary">
				{bundle.job.job_title} · {bundle.job.job_id}
			</div>
			<div class="mt-1 flex flex-wrap gap-2">
				<span class="rounded border border-border/70 px-2 py-0.5">{bundle.job.system}</span>
				<span class="rounded border border-border/70 px-2 py-0.5">{bundle.job.protocol}</span>
				<span class="rounded border border-border/70 px-2 py-0.5">{reviewKind} review</span>
				<span class="rounded border border-border/70 px-2 py-0.5">
					selected run {bundle.selected_review.run_id}
				</span>
			</div>
		</div>

		<div class="grid gap-3 lg:grid-cols-[320px_minmax(0,1fr)]">
			<div class="rounded border border-border bg-bg-secondary">
				<div class="border-b border-border px-3 py-2 text-xs font-medium text-text-secondary">
					Run History ({bundle.runs.length})
				</div>
				<div class="max-h-[72vh] overflow-auto">
					{#if bundle.runs.length === 0}
						<div class="px-3 py-4 text-xs text-text-muted">No runs found for this job.</div>
					{:else}
						{#each bundle.runs as run, index}
							<button
								type="button"
								class="w-full border-b border-border/60 px-3 py-2 text-left transition-colors hover:bg-bg-tertiary/40"
								class:bg-bg-tertiary={index === selectedRunIndex}
								onclick={() => selectRun(index)}
							>
								<div class="font-mono text-[11px] text-text-muted">{run.run_id}</div>
								<div class="mt-0.5 text-xs text-text-primary">
									attempt {run.attempt} · {run.status}
								</div>
								<div class="mt-1 text-[11px] text-text-muted">
									{run.sessions.length} sessions · {run.artifacts.length} artifacts
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
					{#if selectedReviewerDigest}
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

							{#if selectedReviewerDigest.qa.length > 0}
								<div class="space-y-2">
									<div class="text-[11px] uppercase tracking-[0.08em] text-text-muted">Review Q&A</div>
									{#each selectedReviewerDigest.qa as row}
										<div class="rounded border border-border/70 p-2 text-xs">
											<div class="font-medium text-text-primary">{row.question}</div>
											<div class="mt-1 text-text-secondary">{row.answer ?? 'not answered'}</div>
										</div>
									{/each}
								</div>
							{/if}
						</div>
					{/if}
				</div>

				{#if selectedSemanticSummary}
					<div class="rounded border border-border bg-bg-secondary p-3">
						<div class="border-b border-border pb-2 text-xs font-medium text-text-secondary">
							Semantic Summary
						</div>
						<div class="space-y-3 pt-3 text-xs text-text-secondary">
							<div>
								<div class="text-[11px] uppercase tracking-[0.08em] text-text-muted">Changes</div>
								<div class="mt-1 text-text-primary">{selectedSemanticSummary.changes}</div>
							</div>
							<div>
								<div class="text-[11px] uppercase tracking-[0.08em] text-text-muted">Auth / Security</div>
								<div class="mt-1 text-text-primary">{selectedSemanticSummary.auth_security}</div>
							</div>
						</div>
					</div>
				{/if}

				{#if selectedRun}
					<div class="rounded border border-border bg-bg-secondary p-3">
						<div class="border-b border-border pb-2 text-xs font-medium text-text-secondary">
							Artifacts
						</div>
						<div class="space-y-2 pt-3 text-xs">
							{#if selectedRun.artifacts.length === 0}
								<div class="text-text-muted">No artifacts supplied for this run.</div>
							{:else}
								{#each selectedRun.artifacts as artifact}
									<div class="rounded border border-border/70 p-2">
										<div class="font-medium text-text-primary">{artifact.label}</div>
										<div class="mt-1 text-text-secondary">{artifact.kind}</div>
										<a class="mt-1 block break-all text-accent underline" href={artifact.uri}>
											{artifact.uri}
										</a>
									</div>
								{/each}
							{/if}
							{#if bundle.handoff_artifact_uri}
								<div class="rounded border border-border/70 p-2">
									<div class="font-medium text-text-primary">Handoff</div>
									<a class="mt-1 block break-all text-accent underline" href={bundle.handoff_artifact_uri}>
										{bundle.handoff_artifact_uri}
									</a>
								</div>
							{/if}
						</div>
					</div>

					<div class="rounded border border-border bg-bg-secondary p-3">
						<div class="border-b border-border pb-2 text-xs font-medium text-text-secondary">
							Run Sessions ({selectedRun.sessions.length})
						</div>
						<div class="mt-3 flex flex-wrap gap-2">
							{#each selectedRun.sessions as session, index}
								<button
									type="button"
									class="rounded border border-border px-2 py-1 text-xs transition-colors hover:bg-bg-tertiary/40"
									class:bg-bg-tertiary={index === selectedSessionIndex}
									onclick={() => selectSession(index)}
								>
									{session.session.context.title ?? session.session_id}
								</button>
							{/each}
						</div>
					</div>
				{/if}
			</div>
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
		{/if}
	</div>
{/if}
