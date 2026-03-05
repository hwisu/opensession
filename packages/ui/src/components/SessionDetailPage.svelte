<script lang="ts">
import {
	ApiError,
	askSessionChanges,
	changeReaderTextToSpeech,
	getRuntimeSettings,
	getSession,
	getSessionDetail,
	getSessionSemanticSummary,
	readSessionChanges,
	regenerateSessionSemanticSummary,
} from '../api';
import { prepareTimelineEvents } from '../event-helpers';
import {
	buildNativeFilterOptions,
	type SessionViewMode,
	toggleAllBackedFilter,
} from '../session-filters';
import type {
	DesktopChangeReaderScope,
	DesktopSessionSummaryResponse,
	Session,
	SessionDetail,
} from '../types';
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
let changeReaderSupported = $state(false);
let changeReaderEnabled = $state(false);
let changeReaderQaEnabled = $state(false);
let changeReaderScope = $state<DesktopChangeReaderScope>('summary_only');
let changeReaderRuntimeError = $state<string | null>(null);
let changeReaderReading = $state(false);
let changeReaderNarrative = $state<string | null>(null);
let changeReaderReadError = $state<string | null>(null);
let changeReaderVoiceEnabled = $state(false);
let changeReaderVoiceConfigured = $state(false);
let changeReaderVoicePending = $state(false);
let changeReaderVoicePlaying = $state(false);
let changeReaderVoiceError = $state<string | null>(null);
let changeReaderVoiceWarning = $state<string | null>(null);
let changeReaderAudio: HTMLAudioElement | null = null;
let changeReaderAudioUrl: string | null = null;
let changeReaderQuestion = $state('');
let changeReaderAsking = $state(false);
let changeReaderAnswer = $state<string | null>(null);
let changeReaderAskError = $state<string | null>(null);
let changeReaderCitations = $state<string[]>([]);
let changeReaderWarning = $state<string | null>(null);
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

async function handleReadChanges() {
	changeReaderReading = true;
	changeReaderReadError = null;
	changeReaderNarrative = null;
	changeReaderWarning = null;
	changeReaderCitations = [];
	try {
		const payload = await readSessionChanges(sessionId, changeReaderScope);
		changeReaderNarrative = payload.narrative ?? null;
		changeReaderCitations = payload.citations ?? [];
		changeReaderWarning = payload.warning ?? null;
	} catch (e) {
		changeReaderReadError = e instanceof Error ? e.message : 'Failed to read session changes';
	} finally {
		changeReaderReading = false;
	}
}

async function handleAskChangeQuestion() {
	const question = changeReaderQuestion.trim();
	if (!question) {
		changeReaderAskError = 'Ask a question first.';
		return;
	}
	changeReaderAsking = true;
	changeReaderAskError = null;
	changeReaderAnswer = null;
	changeReaderWarning = null;
	try {
		const payload = await askSessionChanges(sessionId, question, changeReaderScope);
		changeReaderAnswer = payload.answer ?? null;
		changeReaderCitations = payload.citations ?? [];
		changeReaderWarning = payload.warning ?? null;
	} catch (e) {
		changeReaderAskError = e instanceof Error ? e.message : 'Failed to answer question';
	} finally {
		changeReaderAsking = false;
	}
}

function releaseChangeReaderAudio() {
	if (changeReaderAudio) {
		changeReaderAudio.pause();
		changeReaderAudio = null;
	}
	if (changeReaderAudioUrl) {
		URL.revokeObjectURL(changeReaderAudioUrl);
		changeReaderAudioUrl = null;
	}
	changeReaderVoicePlaying = false;
}

function decodeBase64Audio(base64: string): Uint8Array {
	const binary = atob(base64);
	const bytes = new Uint8Array(binary.length);
	for (let idx = 0; idx < binary.length; idx += 1) {
		bytes[idx] = binary.charCodeAt(idx);
	}
	return bytes;
}

async function handlePlayChangeReaderVoice() {
	const source = (changeReaderAnswer ?? changeReaderNarrative ?? '').trim();
	if (!source) {
		changeReaderVoiceError = '먼저 Read Changes 또는 Ask를 실행해 주세요.';
		return;
	}
	changeReaderVoicePending = true;
	changeReaderVoiceError = null;
	changeReaderVoiceWarning = null;
	try {
		const payload = await changeReaderTextToSpeech(source, sessionId, changeReaderScope);
		const bytes = decodeBase64Audio(payload.audio_base64);
		const audioBuffer = new ArrayBuffer(bytes.byteLength);
		new Uint8Array(audioBuffer).set(bytes);
		const blob = new Blob([audioBuffer], { type: payload.mime_type || 'audio/mpeg' });
		releaseChangeReaderAudio();
		changeReaderAudioUrl = URL.createObjectURL(blob);
		const audio = new Audio(changeReaderAudioUrl);
		changeReaderAudio = audio;
		audio.onended = () => {
			changeReaderVoicePlaying = false;
		};
		audio.onerror = () => {
			changeReaderVoicePlaying = false;
			changeReaderVoiceError = '음성 재생에 실패했습니다.';
		};
		changeReaderVoicePlaying = true;
		await audio.play();
		if (payload.warning) {
			changeReaderVoiceWarning = payload.warning;
		}
	} catch (e) {
		changeReaderVoiceError = e instanceof Error ? e.message : 'Failed to synthesize voice';
		changeReaderVoicePlaying = false;
	} finally {
		changeReaderVoicePending = false;
	}
}

function handleStopChangeReaderVoice() {
	releaseChangeReaderAudio();
}

$effect(() => {
	loading = true;
	error = null;
	errorCode = null;
	summaryLoading = true;
	summaryError = null;
	semanticSummary = null;
	changeReaderSupported = false;
	changeReaderEnabled = false;
	changeReaderQaEnabled = false;
	changeReaderRuntimeError = null;
	changeReaderNarrative = null;
	changeReaderAnswer = null;
	changeReaderReadError = null;
	changeReaderAskError = null;
	changeReaderWarning = null;
	changeReaderCitations = [];
	changeReaderVoiceEnabled = false;
	changeReaderVoiceConfigured = false;
	changeReaderVoicePending = false;
	changeReaderVoicePlaying = false;
	changeReaderVoiceError = null;
	changeReaderVoiceWarning = null;
	releaseChangeReaderAudio();
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

	getRuntimeSettings()
		.then((runtime) => {
			changeReaderSupported = true;
			changeReaderEnabled = runtime.change_reader?.enabled ?? false;
			changeReaderQaEnabled = runtime.change_reader?.qa_enabled ?? false;
			changeReaderScope = runtime.change_reader?.scope ?? 'summary_only';
			changeReaderVoiceEnabled = runtime.change_reader?.voice?.enabled ?? false;
			changeReaderVoiceConfigured = runtime.change_reader?.voice?.api_key_configured ?? false;
		})
		.catch((e) => {
			changeReaderSupported = false;
			changeReaderRuntimeError =
				e instanceof ApiError && e.status === 501
					? null
					: e instanceof Error
						? e.message
						: 'Failed to load change reader settings';
		});
});

$effect(() => {
	if (!session) return;
	if (initializedForSessionId === session.session_id) return;
	initializeFilters(session);
	initializedForSessionId = session.session_id;
});

$effect(() => {
	return () => {
		releaseChangeReaderAudio();
	};
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
				{#if semanticSummary?.generation_kind === 'heuristic_fallback'}
					<p class="mt-1 text-xs text-warning">Heuristic fallback · 변경 신호가 제한되어 간략 요약을 표시합니다.</p>
				{:else if semanticSummary?.source_kind}
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
					<p class="mt-1 text-xs text-text-muted">변경 신호가 부족해 레이어/파일 요약을 만들지 못했습니다.</p>
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
			{#if semanticSummary?.error && semanticSummary.error !== 'no usable summary signals found'}
				<p class="text-xs text-warning">generation note: {semanticSummary.error}</p>
			{/if}
		{:else}
			<p class="text-xs text-text-muted">No semantic summary generated yet.</p>
		{/if}
	</section>

	{#if changeReaderSupported && changeReaderEnabled}
		<section class="mb-3 space-y-3 rounded border border-border bg-bg-secondary p-3" data-testid="change-reader-card">
			<div class="flex flex-wrap items-center justify-between gap-2">
				<div>
					<p class="text-[11px] uppercase tracking-[0.08em] text-text-muted">Change Reader (Text + Voice)</p>
					<p class="mt-1 text-xs text-text-secondary">Read session changes, ask questions, and play voice narration from local context.</p>
				</div>
				<label class="text-xs text-text-secondary">
					<span class="mr-2">Scope</span>
					<select bind:value={changeReaderScope} class="border border-border bg-bg-primary px-2 py-1 text-xs text-text-primary">
						<option value="summary_only">summary_only</option>
						<option value="full_context">full_context</option>
					</select>
				</label>
			</div>

			<div class="flex flex-wrap items-center gap-2">
				<button
					type="button"
					onclick={handleReadChanges}
					disabled={changeReaderReading}
					data-testid="change-reader-read"
					class="border border-border px-2 py-1 text-xs text-text-secondary hover:text-text-primary disabled:opacity-60"
				>
					{changeReaderReading ? 'Reading…' : 'Read Changes'}
				</button>
				{#if changeReaderVoiceEnabled && changeReaderVoiceConfigured}
					<button
						type="button"
						onclick={handlePlayChangeReaderVoice}
						disabled={changeReaderVoicePending}
						data-testid="change-reader-play-voice"
						class="border border-border px-2 py-1 text-xs text-text-secondary hover:text-text-primary disabled:opacity-60"
					>
						{changeReaderVoicePending ? 'Synthesizing…' : changeReaderVoicePlaying ? 'Replay Voice' : 'Play Voice'}
					</button>
					<button
						type="button"
						onclick={handleStopChangeReaderVoice}
						disabled={!changeReaderVoicePlaying}
						data-testid="change-reader-stop-voice"
						class="border border-border px-2 py-1 text-xs text-text-secondary hover:text-text-primary disabled:opacity-60"
					>
						Stop Voice
					</button>
				{/if}
			</div>
			{#if changeReaderReadError}
				<p class="text-xs text-error">{changeReaderReadError}</p>
			{/if}
			{#if !changeReaderVoiceEnabled}
				<p class="text-xs text-text-muted">Voice is disabled in runtime settings.</p>
			{:else if !changeReaderVoiceConfigured}
				<p class="text-xs text-text-muted">Set Change Reader Voice API key in Settings to enable playback.</p>
			{/if}
			{#if changeReaderVoiceError}
				<p class="text-xs text-error" data-testid="change-reader-voice-error">{changeReaderVoiceError}</p>
			{/if}
			{#if changeReaderVoiceWarning}
				<p class="text-xs text-warning" data-testid="change-reader-voice-warning">{changeReaderVoiceWarning}</p>
			{/if}
			{#if changeReaderNarrative}
				<pre class="overflow-x-auto whitespace-pre-wrap rounded border border-border/70 bg-bg-primary p-2 text-xs text-text-secondary" data-testid="change-reader-narrative">{changeReaderNarrative}</pre>
			{/if}

			{#if changeReaderQaEnabled}
				<div class="space-y-2 border-t border-border/60 pt-2">
					<p class="text-[11px] uppercase tracking-[0.08em] text-text-muted">Q&A</p>
					<div class="flex flex-wrap items-center gap-2">
						<input
							bind:value={changeReaderQuestion}
							data-testid="change-reader-question-input"
							placeholder="Ask about this change..."
							class="min-w-[220px] flex-1 border border-border bg-bg-primary px-2 py-1 text-xs text-text-primary"
						/>
						<button
							type="button"
							onclick={handleAskChangeQuestion}
							disabled={changeReaderAsking}
							data-testid="change-reader-ask"
							class="border border-border px-2 py-1 text-xs text-text-secondary hover:text-text-primary disabled:opacity-60"
						>
							{changeReaderAsking ? 'Asking…' : 'Ask'}
						</button>
					</div>
					{#if changeReaderAskError}
						<p class="text-xs text-error">{changeReaderAskError}</p>
					{/if}
					{#if changeReaderAnswer}
						<pre class="overflow-x-auto whitespace-pre-wrap rounded border border-border/70 bg-bg-primary p-2 text-xs text-text-secondary" data-testid="change-reader-answer">{changeReaderAnswer}</pre>
					{/if}
				</div>
			{:else}
				<p class="text-xs text-text-muted">Q&A is disabled in runtime settings.</p>
			{/if}

			{#if changeReaderCitations.length}
				<p class="text-[11px] text-text-muted" data-testid="change-reader-citations">
					citations: {changeReaderCitations.join(', ')}
				</p>
			{/if}
			{#if changeReaderWarning}
				<p class="text-xs text-warning" data-testid="change-reader-warning">{changeReaderWarning}</p>
			{/if}
		</section>
	{:else if changeReaderRuntimeError}
		<section class="mb-3 rounded border border-border bg-bg-secondary p-3">
			<p class="text-xs text-warning">{changeReaderRuntimeError}</p>
		</section>
	{/if}

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
