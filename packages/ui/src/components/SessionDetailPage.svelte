<script lang="ts">
import {
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
import { appLocale } from '../i18n';
import {
	askSessionChangesSurface,
	loadSessionDetailState,
	readSessionChangesSurface,
	regenerateSessionSummary,
} from '../models/session-detail-model';
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
let detailView = $state<'summary' | 'full'>('full');
let unifiedFilters = $state(new Set<string>());
let branchFilters = $state(new Set<string>());
let nativeFilters = $state(new Set<string>());
let initializedForSessionId = $state<string | null>(null);
const ALL_FILTER_KEY = 'all';
const isKorean = $derived($appLocale === 'ko');

function localize(en: string, ko: string): string {
	return isKorean ? ko : en;
}

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
	const result = await regenerateSessionSummary(
		{
			getSession,
			getSessionDetail,
			getSessionSemanticSummary,
			getRuntimeSettings,
			regenerateSessionSemanticSummary,
			readSessionChanges,
			askSessionChanges,
		},
		sessionId,
	);
	semanticSummary = result.semanticSummary;
	summaryError = result.summaryError;
	summaryRegenerating = false;
}

async function handleReadChanges() {
	changeReaderReading = true;
	changeReaderReadError = null;
	changeReaderNarrative = null;
	changeReaderWarning = null;
	changeReaderCitations = [];
	const result = await readSessionChangesSurface(
		{
			getSession,
			getSessionDetail,
			getSessionSemanticSummary,
			getRuntimeSettings,
			regenerateSessionSemanticSummary,
			readSessionChanges,
			askSessionChanges,
		},
		sessionId,
		changeReaderScope,
	);
	changeReaderNarrative = result.narrative;
	changeReaderCitations = result.citations;
	changeReaderWarning = result.warning;
	changeReaderReadError = result.error;
	changeReaderReading = false;
}

async function handleAskChangeQuestion() {
	changeReaderAsking = true;
	changeReaderAskError = null;
	changeReaderAnswer = null;
	changeReaderWarning = null;
	const result = await askSessionChangesSurface(
		{
			getSession,
			getSessionDetail,
			getSessionSemanticSummary,
			getRuntimeSettings,
			regenerateSessionSemanticSummary,
			readSessionChanges,
			askSessionChanges,
		},
		sessionId,
		changeReaderQuestion,
		changeReaderScope,
	);
	changeReaderAnswer = result.answer;
	changeReaderCitations = result.citations;
	changeReaderWarning = result.warning;
	changeReaderAskError = result.error;
	changeReaderAsking = false;
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
		changeReaderVoiceError = localize(
			'Run Read summary or Ask first.',
			'먼저 변경 내용 읽기 또는 질문을 실행하세요.',
		);
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
			changeReaderVoiceError = localize('Voice playback failed.', '음성 재생에 실패했습니다.');
		};
		changeReaderVoicePlaying = true;
		await audio.play();
		if (payload.warning) {
			changeReaderVoiceWarning = payload.warning;
		}
	} catch (e) {
		changeReaderVoiceError = e instanceof Error ? e.message : localize('Failed to synthesize voice.', '음성 합성에 실패했습니다.');
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
	loadSessionDetailState(
		{
			getSession,
			getSessionDetail,
			getSessionSemanticSummary,
			getRuntimeSettings,
			regenerateSessionSemanticSummary,
			readSessionChanges,
			askSessionChanges,
		},
		sessionId,
	)
		.then((result) => {
			session = result.session;
			detail = result.detail;
			error = result.error;
			errorCode = result.errorCode;
			semanticSummary = result.semanticSummary;
			summaryError = result.summaryError;
			changeReaderSupported = result.changeReaderSupported;
			changeReaderEnabled = result.changeReaderEnabled;
			changeReaderQaEnabled = result.changeReaderQaEnabled;
			changeReaderScope = result.changeReaderScope;
			changeReaderVoiceEnabled = result.changeReaderVoiceEnabled;
			changeReaderVoiceConfigured = result.changeReaderVoiceConfigured;
			changeReaderRuntimeError = result.changeReaderRuntimeError;
			initializedForSessionId = null;
		})
		.finally(() => {
			loading = false;
			summaryLoading = false;
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
	<div class="py-16 text-center text-xs text-text-muted">{localize('Loading session...', '세션을 불러오는 중...')}</div>
{:else if error}
	<div class="py-16 text-center">
		{#if errorCode === 'desktop_contract_mismatch'}
			<p class="text-xs text-error">
				{localize(
					'Desktop runtime and UI bundle contract versions do not match.',
					'데스크톱 런타임과 UI 번들 계약 버전이 일치하지 않습니다.',
				)}
			</p>
			<p class="mt-1 text-xs text-text-muted">
				{localize(
					'Update desktop app/runtime and reopen this session.',
					'데스크톱 앱/런타임을 업데이트한 뒤 이 세션을 다시 열어 주세요.',
				)}
			</p>
		{:else}
			<p class="text-xs text-error">{error}</p>
		{/if}
		<a href="/sessions" class="mt-2 inline-block text-xs text-accent hover:underline">
			{localize('Back to sessions', '세션 목록으로 돌아가기')}
		</a>
	</div>
{:else if session}
	<section class="mb-3 rounded border border-border bg-bg-secondary p-3" data-testid="session-detail-view-switch">
		<div class="flex flex-wrap items-center gap-2">
			<p class="text-[11px] uppercase tracking-[0.08em] text-text-muted">
				{localize('View', '보기')}
			</p>
			<div class="inline-flex rounded border border-border bg-bg-primary p-0.5">
				<button
					type="button"
					class={`px-2 py-1 text-xs ${detailView === 'summary' ? 'bg-accent/20 text-text-primary' : 'text-text-secondary hover:text-text-primary'}`}
					aria-pressed={detailView === 'summary'}
					data-testid="session-detail-view-summary"
					onclick={() => (detailView = 'summary')}
				>
					{localize('Summary', '요약')}
				</button>
				<button
					type="button"
					class={`px-2 py-1 text-xs ${detailView === 'full' ? 'bg-accent/20 text-text-primary' : 'text-text-secondary hover:text-text-primary'}`}
					aria-pressed={detailView === 'full'}
					data-testid="session-detail-view-full"
					onclick={() => (detailView = 'full')}
				>
					{localize('Full', '전체')}
				</button>
			</div>
		</div>
	</section>

	{#if detailView === 'summary'}
		<section class="mb-3 space-y-3 rounded border border-border bg-bg-secondary p-3" data-testid="semantic-summary-card">
			<div class="flex flex-wrap items-center justify-between gap-3">
				<div>
					<p class="text-[11px] uppercase tracking-[0.08em] text-text-muted">
						{localize('Semantic Summary', '시맨틱 요약')}
					</p>
					{#if semanticSummary?.generation_kind === 'heuristic_fallback'}
						<p class="mt-1 text-xs text-warning">
							{localize(
								'Heuristic fallback · limited change signals produced a compact summary.',
								'휴리스틱 대체 · 변경 신호가 제한되어 간략 요약을 표시합니다.',
							)}
						</p>
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
					{summaryRegenerating ? localize('Regenerating…', '다시 생성하는 중...') : localize('Regenerate', '다시 생성')}
				</button>
			</div>

			{#if summaryLoading}
				<p class="text-xs text-text-muted">{localize('Loading semantic summary...', '시맨틱 요약을 불러오는 중...')}</p>
			{:else if summaryError}
				<p class="text-xs text-error">{summaryError}</p>
			{:else if semanticPayload}
				<div class="grid gap-3 md:grid-cols-2">
					<div>
						<p class="text-[11px] uppercase tracking-[0.08em] text-text-muted">{localize('Changes', '변경 사항')}</p>
						<p class="mt-1 text-xs text-text-primary">{semanticPayload.changes ?? localize('(none)', '(없음)')}</p>
					</div>
					<div>
						<p class="text-[11px] uppercase tracking-[0.08em] text-text-muted">{localize('Auth/Security', '인증/보안')}</p>
						<p class="mt-1 text-xs text-text-primary">{semanticPayload.auth_security ?? localize('(none)', '(없음)')}</p>
					</div>
				</div>
				<div>
					<p class="text-[11px] uppercase tracking-[0.08em] text-text-muted">{localize('Layer/File Changes', '레이어/파일 변경')}</p>
					{#if semanticPayload.layer_file_changes?.length}
						<div class="mt-2 space-y-2">
							{#each semanticPayload.layer_file_changes as item}
								<div class="rounded border border-border/70 p-2">
									<div class="text-xs font-medium text-text-primary">{item.layer ?? localize('(unknown layer)', '(알 수 없는 레이어)')}</div>
									<div class="mt-1 text-xs text-text-secondary">{item.summary ?? ''}</div>
									{#if item.files?.length}
										<div class="mt-1 text-[11px] text-text-muted">{item.files.join(', ')}</div>
									{/if}
								</div>
							{/each}
						</div>
					{:else}
						<p class="mt-1 text-xs text-text-muted">
							{localize(
								'Not enough change signals were available to build a layer/file summary.',
								'변경 신호가 부족해 레이어/파일 요약을 만들지 못했습니다.',
							)}
						</p>
					{/if}
				</div>
				{#if semanticDiffTree.length}
					<div>
						<p class="text-[11px] uppercase tracking-[0.08em] text-text-muted">{localize('Diff Tree', 'Diff 트리')}</p>
						<div class="mt-2 space-y-2">
							{#each semanticDiffTree as layer}
								<details class="rounded border border-border/70 p-2" open={layer.file_count != null && layer.file_count <= 5}>
									<summary class="cursor-pointer text-xs text-text-primary">
										{layer.layer ?? localize('(layer)', '(레이어)')} · {localize('files', '파일')}={layer.file_count ?? 0} · +{layer.lines_added ?? 0}/-{layer.lines_removed ?? 0}
									</summary>
									<div class="mt-2 space-y-2">
										{#each layer.files ?? [] as file}
											<details class="rounded border border-border/60 p-2" open={!file.is_large}>
												<summary class="cursor-pointer text-[11px] text-text-secondary">
													{file.path ?? localize('(file)', '(파일)')} [{file.operation ?? localize('edit', '수정')}] +{file.lines_added ?? 0}/-{file.lines_removed ?? 0}
													{#if file.is_large}
														· {localize('large', '대형')}
													{/if}
												</summary>
												{#if file.hunks?.length}
													<div class="mt-2 space-y-1">
														{#each file.hunks as hunk}
															<div class="rounded border border-border/50 bg-bg-primary p-2">
																<div class="text-[11px] font-mono text-text-muted">{hunk.header ?? localize('(hunk)', '(청크)')}</div>
																{#if hunk.lines?.length}
																	<pre class="mt-1 overflow-x-auto whitespace-pre-wrap text-[11px] text-text-secondary">{hunk.lines.join('\n')}</pre>
																{/if}
																{#if (hunk.omitted_lines ?? 0) > 0}
																	<div class="mt-1 text-[11px] text-text-muted">
																		{isKorean ? `… ${hunk.omitted_lines}줄 생략` : `… ${hunk.omitted_lines} lines omitted`}
																	</div>
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
					<p class="text-xs text-warning">{localize('generation note:', '생성 메모:')} {semanticSummary.error}</p>
				{/if}
			{:else}
				<p class="text-xs text-text-muted">{localize('No semantic summary generated yet.', '아직 시맨틱 요약이 생성되지 않았습니다.')}</p>
			{/if}
		</section>

		{#if changeReaderSupported && changeReaderEnabled}
			<section class="mb-3 space-y-3 rounded border border-border bg-bg-secondary p-3" data-testid="change-reader-card">
				<div class="flex flex-wrap items-center justify-between gap-2">
					<div>
						<p class="text-[11px] uppercase tracking-[0.08em] text-text-muted">{localize('Change Reader (Text + Voice)', '변경 리더 (텍스트 + 음성)')}</p>
						<p class="mt-1 text-xs text-text-secondary">
							{localize(
								'Read session changes, ask questions, and play voice narration from local context.',
								'로컬 컨텍스트에서 세션 변경을 읽고, 질문하고, 음성 내레이션을 재생합니다.',
							)}
						</p>
					</div>
					<label class="text-xs text-text-secondary">
						<span class="mr-2">{localize('Scope', '범위')}</span>
						<select bind:value={changeReaderScope} class="border border-border bg-bg-primary px-2 py-1 text-xs text-text-primary">
							<option value="summary_only">{localize('Summary only', '요약만')}</option>
							<option value="full_context">{localize('Full context', '전체 컨텍스트')}</option>
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
						{changeReaderReading ? localize('Reading…', '읽는 중...') : localize('Read summary', '변경 내용 읽기')}
					</button>
					{#if changeReaderVoiceEnabled && changeReaderVoiceConfigured}
						<button
							type="button"
							onclick={handlePlayChangeReaderVoice}
							disabled={changeReaderVoicePending}
							data-testid="change-reader-play-voice"
							class="border border-border px-2 py-1 text-xs text-text-secondary hover:text-text-primary disabled:opacity-60"
						>
							{changeReaderVoicePending
								? localize('Synthesizing…', '합성 중...')
								: changeReaderVoicePlaying
									? localize('Replay Voice', '음성 다시 재생')
									: localize('Play Voice', '음성 재생')}
						</button>
						<button
							type="button"
							onclick={handleStopChangeReaderVoice}
							disabled={!changeReaderVoicePlaying}
							data-testid="change-reader-stop-voice"
							class="border border-border px-2 py-1 text-xs text-text-secondary hover:text-text-primary disabled:opacity-60"
						>
							{localize('Stop Voice', '음성 중지')}
						</button>
					{/if}
				</div>
				{#if changeReaderReadError}
					<p class="text-xs text-error">{changeReaderReadError}</p>
				{/if}
				{#if !changeReaderVoiceEnabled}
					<p class="text-xs text-text-muted">{localize('Voice is disabled in runtime settings.', '런타임 설정에서 음성이 비활성화되어 있습니다.')}</p>
				{:else if !changeReaderVoiceConfigured}
					<p class="text-xs text-text-muted">
						{localize(
							'Set Change Reader Voice API key in Settings to enable playback.',
							'설정에서 Change Reader Voice API 키를 입력하면 재생을 사용할 수 있습니다.',
						)}
					</p>
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
						<p class="text-[11px] uppercase tracking-[0.08em] text-text-muted">
							{localize('Questions', '질문')}
						</p>
						<div class="flex flex-wrap items-center gap-2">
							<input
								bind:value={changeReaderQuestion}
								data-testid="change-reader-question-input"
								placeholder={localize('Ask about this change...', '이 변경에 대해 질문하세요...')}
								class="min-w-[220px] flex-1 border border-border bg-bg-primary px-2 py-1 text-xs text-text-primary"
							/>
							<button
								type="button"
								onclick={handleAskChangeQuestion}
								disabled={changeReaderAsking}
								data-testid="change-reader-ask"
								class="border border-border px-2 py-1 text-xs text-text-secondary hover:text-text-primary disabled:opacity-60"
							>
								{changeReaderAsking ? localize('Asking…', '질문 중...') : localize('Ask', '질문하기')}
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
					<p class="text-xs text-text-muted">
						{localize(
							'Questions are disabled in runtime settings.',
							'런타임 설정에서 질문 기능이 비활성화되어 있습니다.',
						)}
					</p>
				{/if}

				{#if changeReaderCitations.length}
					<p class="text-[11px] text-text-muted" data-testid="change-reader-citations">
						{localize('citations:', '인용:')} {changeReaderCitations.join(', ')}
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
	{:else}
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
{/if}
