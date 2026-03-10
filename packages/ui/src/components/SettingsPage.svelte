<script lang="ts">
import {
	ApiError,
	createGitCredential,
	deleteGitCredential,
	detectSummaryProvider,
	getApiCapabilities,
	getLifecycleCleanupStatus,
	getRuntimeSettings,
	getSummaryBatchStatus,
	getSettings,
	isAuthenticated,
	issueApiKey,
	listGitCredentials,
	runSummaryBatch,
	updateRuntimeSettings,
	vectorIndexRebuild,
	vectorIndexStatus,
	vectorInstallModel,
	vectorPreflight,
} from '../api';
import type {
	DesktopChangeReaderScope,
	DesktopChangeReaderVoiceProvider,
	DesktopLifecycleCleanupStatusResponse,
	DesktopRuntimeSettingsResponse,
	DesktopSummaryBatchExecutionMode,
	DesktopSummaryBatchScope,
	DesktopSummaryBatchStatusResponse,
	DesktopSummaryOutputShape,
	DesktopSummaryProviderDetectResponse,
	DesktopSummaryProviderId,
	DesktopSummaryProviderTransport,
	DesktopSummaryResponseStyle,
	DesktopSummarySourceMode,
	DesktopSummaryStorageBackend,
	DesktopSummaryTriggerMode,
	DesktopVectorIndexStatusResponse,
	DesktopVectorChunkingMode,
	DesktopVectorPreflightResponse,
	DesktopVectorSearchGranularity,
	DesktopVectorSearchProvider,
	GitCredentialSummary,
	UserSettings,
} from '../types';
import FieldHelp from './FieldHelp.svelte';
import FloatingJobStatus from './FloatingJobStatus.svelte';
import RuntimeActivityPanel from './settings-page/RuntimeActivityPanel.svelte';
import RuntimeQuickMenu from './settings-page/RuntimeQuickMenu.svelte';
import SettingsSectionNav from './settings-page/SettingsSectionNav.svelte';
import type {
	RuntimeActivityCard,
	RuntimeActivityTone,
	RuntimeQuickJumpLink,
	SettingsSectionNavItem,
} from './settings-page/models';
import { appLocale } from '../i18n';
import LanguageSettingsPanel from './LanguageSettingsPanel.svelte';
import {
	copyTextSurface,
	loadGitCredentialsState,
	loadRuntimeSettingsState,
	loadSettingsPageState,
	nextSettingsBackgroundPollDelay,
} from '../models/settings-model';

const {
	onNavigate = (path: string) => {
		window.location.assign(path);
	},
}: {
	onNavigate?: (path: string) => void;
} = $props();

const isKorean = $derived($appLocale === 'ko');

function localize(en: string, ko: string): string {
	return isKorean ? ko : en;
}

function boolWord(value: boolean): string {
	return value ? localize('yes', '예') : localize('no', '아니오');
}

function toggleWord(value: boolean): string {
	return value ? localize('On', '켜짐') : localize('Off', '꺼짐');
}

let settings = $state<UserSettings | null>(null);
let loading = $state(true);
let error = $state<string | null>(null);
let authApiEnabled = $state(true);
let issuing = $state(false);
let issuedApiKey = $state<string | null>(null);
let copyMessage = $state<string | null>(null);
let authRequired = $state(false);
let credentials = $state<GitCredentialSummary[]>([]);
let credentialsLoading = $state(false);
let credentialsError = $state<string | null>(null);
let credentialsSupported = $state(true);
let creatingCredential = $state(false);
let deletingCredentialId = $state<string | null>(null);

let runtimeSettings = $state<DesktopRuntimeSettingsResponse | null>(null);
let runtimeLoading = $state(false);
let runtimeSaving = $state(false);
let runtimeDetecting = $state(false);
let runtimeSupported = $state(true);
let runtimeError = $state<string | null>(null);
let runtimeDetectMessage = $state<string | null>(null);
let runtimeSessionDefaultView = $state<'full' | 'compressed'>('full');
let runtimeProvider = $state<DesktopSummaryProviderId>('disabled');
let runtimeProviderTransport = $state<DesktopSummaryProviderTransport>('none');
let runtimeEndpoint = $state('');
let runtimeModel = $state('');
let runtimeSourceMode = $state<DesktopSummarySourceMode>('session_only');
let runtimeResponseStyle = $state<DesktopSummaryResponseStyle>('standard');
let runtimeOutputShape = $state<DesktopSummaryOutputShape>('layered');
let runtimePromptTemplate = $state('');
let runtimePromptDefaultTemplate = $state('');
let runtimeTriggerMode = $state<DesktopSummaryTriggerMode>('on_session_save');
let runtimeStorageBackend = $state<DesktopSummaryStorageBackend>('hidden_ref');
let runtimeBatchExecutionMode = $state<DesktopSummaryBatchExecutionMode>('on_app_start');
let runtimeBatchScope = $state<DesktopSummaryBatchScope>('recent_days');
let runtimeBatchRecentDays = $state(30);
let runtimeVectorEnabled = $state(false);
let runtimeVectorProvider = $state<DesktopVectorSearchProvider>('ollama');
let runtimeVectorModel = $state('bge-m3');
let runtimeVectorEndpoint = $state('http://127.0.0.1:11434');
let runtimeVectorGranularity = $state<DesktopVectorSearchGranularity>('event_line_chunk');
let runtimeVectorChunkingMode = $state<DesktopVectorChunkingMode>('auto');
let runtimeVectorChunkSizeLines = $state(12);
let runtimeVectorChunkOverlapLines = $state(3);
let runtimeVectorTopKChunks = $state(30);
let runtimeVectorTopKSessions = $state(20);
let runtimeChangeReaderEnabled = $state(false);
let runtimeChangeReaderScope = $state<DesktopChangeReaderScope>('summary_only');
let runtimeChangeReaderQaEnabled = $state(true);
let runtimeChangeReaderMaxContextChars = $state(12000);
let runtimeChangeReaderVoiceEnabled = $state(false);
let runtimeChangeReaderVoiceProvider = $state<DesktopChangeReaderVoiceProvider>('openai');
let runtimeChangeReaderVoiceModel = $state('gpt-4o-mini-tts');
let runtimeChangeReaderVoiceName = $state('alloy');
let runtimeChangeReaderVoiceApiKey = $state('');
let runtimeChangeReaderVoiceApiKeyConfigured = $state(false);
let runtimeLifecycleEnabled = $state(true);
let runtimeSessionTtlDays = $state(30);
let runtimeSummaryTtlDays = $state(30);
let runtimeCleanupIntervalSecs = $state(3600);
let runtimeVectorPreflight = $state<DesktopVectorPreflightResponse | null>(null);
let runtimeVectorIndex = $state<DesktopVectorIndexStatusResponse | null>(null);
let runtimeVectorInstalling = $state(false);
let runtimeVectorReindexing = $state(false);
let runtimeVectorError = $state<string | null>(null);
let runtimeSummaryBatchStatus = $state<DesktopSummaryBatchStatusResponse | null>(null);
let runtimeSummaryBatchRunning = $state(false);
let runtimeLifecycleStatus = $state<DesktopLifecycleCleanupStatusResponse | null>(null);
const BACKGROUND_JOB_POLL_INTERVAL_MS = 1000;
const BACKGROUND_STATUS_POLL_INTERVAL_MS = 5000;

type RuntimeDraftSnapshot = {
	session_default_view: 'full' | 'compressed';
	summary: {
		provider: {
			id: DesktopSummaryProviderId;
			endpoint: string;
			model: string;
		};
		prompt: {
			template: string;
		};
		response: {
			style: DesktopSummaryResponseStyle;
			shape: DesktopSummaryOutputShape;
		};
		storage: {
			trigger: DesktopSummaryTriggerMode;
			backend: DesktopSummaryStorageBackend;
		};
		source_mode: DesktopSummarySourceMode;
		batch: {
			execution_mode: DesktopSummaryBatchExecutionMode;
			scope: DesktopSummaryBatchScope;
			recent_days: number;
		};
	};
	vector_search: {
		enabled: boolean;
		provider: DesktopVectorSearchProvider;
		model: string;
		endpoint: string;
		granularity: DesktopVectorSearchGranularity;
		chunking_mode: DesktopVectorChunkingMode;
		chunk_size_lines: number;
		chunk_overlap_lines: number;
		top_k_chunks: number;
		top_k_sessions: number;
	};
	change_reader: {
		enabled: boolean;
		scope: DesktopChangeReaderScope;
		qa_enabled: boolean;
		max_context_chars: number;
		voice: {
			enabled: boolean;
			provider: DesktopChangeReaderVoiceProvider;
			model: string;
			voice: string;
			api_key_pending: boolean;
		};
	};
	lifecycle: {
		enabled: boolean;
		session_ttl_days: number;
		summary_ttl_days: number;
		cleanup_interval_secs: number;
	};
};

function isVectorInstallRunning(status: DesktopVectorPreflightResponse | null): boolean {
	return status?.install_state === 'installing';
}

function isVectorIndexRunning(status: DesktopVectorIndexStatusResponse | null): boolean {
	return status?.state === 'running';
}

function isSummaryBatchRunning(status: DesktopSummaryBatchStatusResponse | null): boolean {
	return status?.state === 'running';
}

function isLifecycleCleanupRunning(
	status: DesktopLifecycleCleanupStatusResponse | null,
): boolean {
	return status?.state === 'running';
}

function progressPercent(processed: number, total: number): number | null {
	if (total <= 0) return null;
	return Math.min(100, Math.max(0, Math.round((processed / total) * 100)));
}

function vectorIndexProgressLabel(status: DesktopVectorIndexStatusResponse | null): string | null {
	if (!status || status.total_sessions <= 0) return null;
	const pct = progressPercent(status.processed_sessions, status.total_sessions);
	if (pct == null) return null;
	return `${status.processed_sessions}/${status.total_sessions} ${localize('sessions', '세션')} (${pct}%)`;
}

function summaryBatchProgressLabel(
	status: DesktopSummaryBatchStatusResponse | null,
): string | null {
	if (!status) return null;
	if (status.total_sessions <= 0) {
		return status.failed_sessions > 0
			? localize(`failed ${status.failed_sessions}`, `${status.failed_sessions}개 실패`)
			: localize('no queued sessions', '대기 중인 세션이 없습니다');
	}
	return localize(
		`${status.processed_sessions}/${status.total_sessions} sessions · failed ${status.failed_sessions}`,
		`${status.processed_sessions}/${status.total_sessions} 세션 · 실패 ${status.failed_sessions}`,
	);
}

function formatIntervalSeconds(seconds: number): string {
	if (seconds < 60) return localize(`${seconds}s`, `${seconds}초`);
	if (seconds < 3600) return localize(`${Math.floor(seconds / 60)}m`, `${Math.floor(seconds / 60)}분`);
	if (seconds % 3600 === 0) {
		return localize(`${Math.floor(seconds / 3600)}h`, `${Math.floor(seconds / 3600)}시간`);
	}
	return localize(
		`${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`,
		`${Math.floor(seconds / 3600)}시간 ${Math.floor((seconds % 3600) / 60)}분`,
	);
}

function lifecycleResultLabel(status: DesktopLifecycleCleanupStatusResponse | null): string {
	if (!status) return localize('No lifecycle cleanup runs recorded yet.', '아직 수명주기 정리 실행 기록이 없습니다.');
	return localize(
		`${status.deleted_sessions} sessions deleted · ${status.deleted_summaries} summaries removed`,
		`세션 ${status.deleted_sessions}개 삭제 · 요약 ${status.deleted_summaries}개 제거`,
	);
}

function lifecycleNextRunLabel(): string {
	if (!runtimeLifecycleEnabled) return localize('paused', '일시중지');
	if (isLifecycleCleanupRunning(runtimeLifecycleStatus)) return localize('running now', '지금 실행 중');
	const anchor = runtimeLifecycleStatus?.finished_at ?? runtimeLifecycleStatus?.started_at;
	if (!anchor) {
		return localize(
			`after app start, then every ${formatIntervalSeconds(runtimeCleanupIntervalSecs)}`,
			`앱 시작 후, 이후 ${formatIntervalSeconds(runtimeCleanupIntervalSecs)}마다`,
		);
	}
	const next = new Date(new Date(anchor).getTime() + runtimeCleanupIntervalSecs * 1000);
	if (Number.isNaN(next.getTime())) {
		return localize(
			`every ${formatIntervalSeconds(runtimeCleanupIntervalSecs)}`,
			`${formatIntervalSeconds(runtimeCleanupIntervalSecs)}마다`,
		);
	}
	return formatDate(next.toISOString());
}

function activityStateTone(state: string | null | undefined): RuntimeActivityTone {
	if (state === 'running') return 'running';
	if (state === 'failed') return 'failed';
	if (state === 'complete') return 'complete';
	return 'disabled';
}

const runtimeChangeReaderQaToggleDisabled = $derived.by(
	() => !runtimeChangeReaderQaEnabled && !runtimeChangeReaderEnabled,
);

const runtimeChangeReaderVoiceApiKeyReady = $derived.by(
	() => runtimeChangeReaderVoiceApiKeyConfigured || runtimeChangeReaderVoiceApiKey.trim().length > 0,
);

const runtimeChangeReaderVoiceToggleDisabled = $derived.by(() => {
	if (runtimeChangeReaderVoiceEnabled) return false;
	if (!runtimeChangeReaderEnabled) return true;
	return !runtimeChangeReaderVoiceApiKeyReady;
});

const runtimeChangeReaderVoiceBlockedReason = $derived.by(() => {
	if (runtimeChangeReaderVoiceEnabled) return null;
	if (!runtimeChangeReaderEnabled) return localize('Enable Change Reader first.', '먼저 변경 리더를 켜세요.');
	if (!runtimeChangeReaderVoiceApiKeyReady) return localize('Add a Voice API key first.', '먼저 Voice API 키를 추가하세요.');
	return null;
});

const runtimeChangeReaderVoiceKeyStatusLabel = $derived.by(() => {
	if (runtimeChangeReaderVoiceApiKey.trim().length > 0) {
		return localize('Voice API key: pending save', 'Voice API 키: 저장 대기 중');
	}
	return runtimeChangeReaderVoiceApiKeyConfigured
		? localize('Voice API key: configured', 'Voice API 키: 설정됨')
		: localize('Voice API key: missing', 'Voice API 키: 없음');
});

const runtimeChangeReaderVoiceHint = $derived.by(() => {
	if (runtimeChangeReaderVoiceBlockedReason) {
		return localize(
			`${runtimeChangeReaderVoiceBlockedReason} Voice playback only reads the change reader output aloud.`,
			`${runtimeChangeReaderVoiceBlockedReason} 음성 재생은 변경 리더 출력을 소리 내어 읽기만 합니다.`,
		);
	}
	return localize(
		'Voice playback reads the same change reader output aloud. It does not change summaries or follow-up questions.',
		'음성 재생은 같은 변경 리더 출력을 소리 내어 읽습니다. 요약이나 후속 질문 내용은 바꾸지 않습니다.',
	);
});

const runtimeChangeReaderVoiceSummary = $derived.by(() => {
	const base = `${runtimeChangeReaderVoiceProvider} · ${runtimeChangeReaderVoiceModel}`;
	if (!runtimeChangeReaderVoiceApiKeyReady) {
		return localize(`${base} · API key required`, `${base} · API 키 필요`);
	}
	return base;
});

const floatingJobs = $derived.by(() => {
	const jobs: Array<{ id: string; label: string; detail: string }> = [];
	if (runtimeSaving) {
		jobs.push({
			id: 'runtime-save',
			label: localize('Saving runtime settings', '런타임 설정 저장 중'),
			detail: localize(
				'Storage migration and runtime validation can take a while. Continue using the page.',
				'저장소 마이그레이션과 런타임 검증에 시간이 걸릴 수 있습니다. 이 페이지는 계속 사용해도 됩니다.',
			),
		});
	}
	if (runtimeVectorInstalling) {
		jobs.push({
			id: 'vector-install',
			label: localize('Installing vector model', '벡터 모델 설치 중'),
			detail: localize('Model pull is running in background.', '모델 다운로드가 백그라운드에서 진행 중입니다.'),
		});
	}
	if (runtimeVectorReindexing) {
		const progress = vectorIndexProgressLabel(runtimeVectorIndex);
		jobs.push({
			id: 'vector-reindex',
			label: localize('Rebuilding vector index', '벡터 인덱스 재구성 중'),
			detail: progress
				? localize(
					`Session embeddings are being rebuilt in background. Processed ${progress}.`,
					`세션 임베딩을 백그라운드에서 다시 만들고 있습니다. 진행 ${progress}.`,
				)
				: localize(
					'Session embeddings are being rebuilt in background.',
					'세션 임베딩을 백그라운드에서 다시 만들고 있습니다.',
				),
		});
	}
	if (runtimeSummaryBatchRunning) {
		const progress = summaryBatchProgressLabel(runtimeSummaryBatchStatus);
		jobs.push({
			id: 'summary-batch',
			label: localize('Running summary batch', '요약 배치 실행 중'),
			detail: progress
				? localize(
					`Generating summaries in background. ${progress}.`,
					`백그라운드에서 요약을 생성하고 있습니다. ${progress}.`,
				)
				: localize('Generating summaries in background.', '백그라운드에서 요약을 생성하고 있습니다.'),
		});
	}
	if (isLifecycleCleanupRunning(runtimeLifecycleStatus)) {
		jobs.push({
			id: 'lifecycle-cleanup',
			label: localize('Running lifecycle cleanup', '수명주기 정리 실행 중'),
			detail:
				runtimeLifecycleStatus?.message ??
				localize('Removing expired sessions and summaries.', '만료된 세션과 요약을 제거하고 있습니다.'),
		});
	}
	return jobs;
});

function providerTransportForId(id: DesktopSummaryProviderId): DesktopSummaryProviderTransport {
	if (id === 'ollama') return 'http';
	if (id === 'codex_exec' || id === 'claude_cli') return 'cli';
	return 'none';
}

function currentRuntimeProviderTransport(): DesktopSummaryProviderTransport {
	if (runtimeProvider === 'disabled') return 'none';
	return providerTransportForId(runtimeProvider);
}

function storageBackendLabel(backend: DesktopSummaryStorageBackend): string {
	if (backend === 'hidden_ref') return localize('git hidden refs', 'git 숨김 ref');
	if (backend === 'local_db') return localize('local SQLite', '로컬 SQLite');
	return localize('ephemeral only', '임시 전용');
}

function persistedStorageBackend(): DesktopSummaryStorageBackend | null {
	return runtimeSettings?.summary.storage.backend ?? null;
}

function persistedStorageBackendLabel(): string | null {
	const current = persistedStorageBackend();
	return current ? storageBackendLabel(current) : null;
}

function hasPendingStorageBackendChange(): boolean {
	const current = persistedStorageBackend();
	return current != null && current !== runtimeStorageBackend;
}

function storageBackendSummary(backend: DesktopSummaryStorageBackend): string {
	if (backend === 'hidden_ref') {
		return localize(
			'Read and write persisted summaries from git hidden refs in each session repository.',
			'각 세션 저장소의 git 숨김 ref에서 저장된 요약을 읽고 씁니다.',
		);
	}
	if (backend === 'local_db') {
		return localize(
			'Read and write persisted summaries from the local SQLite table `session_semantic_summaries`.',
			'로컬 SQLite 테이블 `session_semantic_summaries`에서 저장된 요약을 읽고 씁니다.',
		);
	}
	return localize(
		'Do not read or write persisted summaries. Results are generated only for the current request.',
		'저장된 요약을 읽거나 쓰지 않습니다. 결과는 현재 요청에 대해서만 생성됩니다.',
	);
}

function storageBackendDetails(backend: DesktopSummaryStorageBackend): string {
	if (backend === 'hidden_ref') {
		return localize(
			'Best when the session belongs to a git repository and you want git-backed summary history alongside the repo.',
			'세션이 git 저장소에 속하고, 저장소와 함께 git 기반 요약 이력을 유지하고 싶을 때 적합합니다.',
		);
	}
	if (backend === 'local_db') {
		return localize(
			'Best when you want machine-local persistence without writing anything into git refs.',
			'git ref에는 아무것도 쓰지 않고, 현재 머신에만 저장하고 싶을 때 적합합니다.',
		);
	}
	return localize(
		'Use this only when you want no persistence. Existing stored summaries are left where they already are.',
		'저장을 전혀 원하지 않을 때만 사용하세요. 이미 저장된 요약은 기존 위치에 그대로 남습니다.',
	);
}

function storageBackendTransitionDetail(): string {
	const current = persistedStorageBackend();
	if (!current) {
		return localize('Load runtime settings to inspect storage migration behavior.', '저장소 마이그레이션 동작을 보려면 런타임 설정을 먼저 불러오세요.');
	}
	if (current === runtimeStorageBackend) {
		return localize(
			'No storage backend switch is pending. Click Save Runtime only if you want to persist other runtime edits.',
			'대기 중인 저장소 백엔드 변경이 없습니다. 다른 런타임 수정 사항을 저장하려는 경우에만 런타임 저장을 누르세요.',
		);
	}
	if (current === 'none') {
		return localize(
			`On next save, new summaries will persist to ${storageBackendLabel(runtimeStorageBackend)}. Nothing is copied because the current backend stores no persisted summaries.`,
			`다음 저장부터 새 요약은 ${storageBackendLabel(runtimeStorageBackend)}에 저장됩니다. 현재 백엔드에는 저장된 요약이 없어서 복사되는 항목은 없습니다.`,
		);
	}
	if (runtimeStorageBackend === 'none') {
		return localize(
			`On next save, desktop stops reading and writing persisted summaries. Existing summaries stay in ${storageBackendLabel(current)}. Nothing is migrated or deleted automatically.`,
			`다음 저장부터 데스크톱은 저장된 요약을 읽거나 쓰지 않습니다. 기존 요약은 ${storageBackendLabel(current)}에 그대로 남고, 자동 마이그레이션이나 삭제는 일어나지 않습니다.`,
		);
	}
	return localize(
		`On next save, existing summaries are copied from ${storageBackendLabel(current)} to ${storageBackendLabel(runtimeStorageBackend)}. Existing source copies are kept.`,
		`다음 저장 시 기존 요약이 ${storageBackendLabel(current)}에서 ${storageBackendLabel(runtimeStorageBackend)}로 복사됩니다. 기존 원본 복사본도 유지됩니다.`,
	);
}

function runtimeSaveLabel(): string {
	if (runtimeSaving) return localize('Saving...', '저장 중...');
	const current = persistedStorageBackend();
	if (!current || current === runtimeStorageBackend) {
		return localize('Save Runtime', '런타임 저장');
	}
	if (
		(current === 'hidden_ref' && runtimeStorageBackend === 'local_db') ||
		(current === 'local_db' && runtimeStorageBackend === 'hidden_ref')
	) {
		return localize('Save Runtime + Migrate', '런타임 저장 + 마이그레이션');
	}
	return localize('Save Runtime + Apply Storage', '런타임 저장 + 저장소 적용');
}

let activeSettingsSectionId = $state('settings-section-overview');

function getSettingsScrollContainer(): HTMLElement | Window | null {
	if (typeof document === 'undefined') return null;
	const main = document.querySelector('main');
	return main instanceof HTMLElement ? main : window;
}

function scrollToSettingsSection(sectionId: string) {
	if (typeof document === 'undefined') return;
	const section = document.getElementById(sectionId);
	if (!section) return;
	const scrollContainer = getSettingsScrollContainer();
	if (!scrollContainer) return;
	if (scrollContainer instanceof Window) {
		const top = window.scrollY + section.getBoundingClientRect().top - 16;
		window.scrollTo({ top: Math.max(0, top), behavior: 'auto' });
		return;
	}
	const top =
		scrollContainer.scrollTop +
		section.getBoundingClientRect().top -
		scrollContainer.getBoundingClientRect().top -
		16;
	scrollContainer.scrollTo({ top: Math.max(0, top), behavior: 'auto' });
}

function setActiveSettingsSection(sectionId: string) {
	activeSettingsSectionId = sectionId;
	scrollToSettingsSection(sectionId);
}

function updateRuntimeProvider(provider: DesktopSummaryProviderId) {
	runtimeProvider = provider;
	handleRuntimeProviderChange();
}

function updateRuntimeStorageBackend(backend: DesktopSummaryStorageBackend) {
	runtimeStorageBackend = backend;
}

function toggleRuntimeSummaryTrigger() {
	runtimeTriggerMode = runtimeTriggerMode === 'on_session_save' ? 'manual' : 'on_session_save';
}

function toggleRuntimeBatchExecution() {
	runtimeBatchExecutionMode =
		runtimeBatchExecutionMode === 'on_app_start' ? 'manual' : 'on_app_start';
}

function toggleRuntimeLifecycle() {
	runtimeLifecycleEnabled = !runtimeLifecycleEnabled;
}

function toggleRuntimeVector() {
	runtimeVectorEnabled = !runtimeVectorEnabled;
}

function toggleRuntimeChangeReader() {
	runtimeChangeReaderEnabled = !runtimeChangeReaderEnabled;
}

function toggleRuntimeChangeReaderQa() {
	runtimeChangeReaderQaEnabled = !runtimeChangeReaderQaEnabled;
}

function toggleRuntimeChangeReaderVoice() {
	runtimeChangeReaderVoiceEnabled = !runtimeChangeReaderVoiceEnabled;
}

const runtimeQuickJumpLinks: RuntimeQuickJumpLink[] = [
	{ id: 'runtime-section-activity', label: localize('Activity', '활동') },
	{ id: 'runtime-section-provider', label: localize('Provider', '프로바이더') },
	{ id: 'runtime-section-vector', label: localize('Vector', '벡터') },
	{ id: 'runtime-section-change-reader', label: localize('Reader', '리더') },
	{ id: 'runtime-section-storage', label: localize('Storage', '저장소') },
	{ id: 'runtime-section-summary-batch', label: localize('Batch', '배치') },
	{ id: 'runtime-section-lifecycle', label: 'TTL' },
] as const;

const settingsNavItems = $derived.by((): SettingsSectionNavItem[] => {
	const items = [
		{
			id: 'settings-section-overview',
			label: localize('Overview', '개요'),
			detail: localize('Page summary and account context', '페이지 요약과 계정 상태'),
			visible: true,
		},
		{
			id: 'settings-section-profile',
			label: localize('Profile', '프로필'),
			detail: localize('Identity and linked providers', '신원과 연결된 프로바이더'),
			visible: authApiEnabled && !authRequired,
		},
		{
			id: 'settings-section-api-key',
			label: localize('API Key', 'API 키'),
			detail: localize('CLI and automation access', 'CLI 및 자동화 접근'),
			visible: authApiEnabled && !authRequired,
		},
		{
			id: 'settings-section-git-credentials',
			label: localize('Git Auth', 'Git 인증'),
			detail: localize('Private repository credentials', '비공개 저장소 자격 증명'),
			visible: authApiEnabled && !authRequired,
		},
		{
			id: 'settings-section-runtime',
			label: localize('Runtime', '런타임'),
			detail: localize('Desktop summary controls', '데스크톱 요약 제어'),
			visible: true,
		},
		{
			id: 'runtime-section-activity',
			label: localize('Activity', '활동'),
			detail: localize('Live job and cleanup status', '실시간 작업 및 정리 상태'),
			visible: runtimeSupported,
		},
		{
			id: 'runtime-section-provider',
			label: localize('Provider', '프로바이더'),
			detail: localize('Summary backend and transport', '요약 백엔드와 전송 방식'),
			visible: runtimeSupported,
		},
		{
			id: 'runtime-section-prompt',
			label: localize('Prompt', '프롬프트'),
			detail: localize('Template and reset controls', '템플릿과 초기화 제어'),
			visible: runtimeSupported,
		},
		{
			id: 'runtime-section-response',
			label: localize('Response', '응답'),
			detail: localize('Style, shape, preview', '스타일, 형태, 미리보기'),
			visible: runtimeSupported,
		},
		{
			id: 'runtime-section-vector',
			label: localize('Vector', '벡터'),
			detail: localize('Embeddings and index jobs', '임베딩과 인덱스 작업'),
			visible: runtimeSupported,
		},
		{
			id: 'runtime-section-change-reader',
			label: localize('Reader', '리더'),
			detail: localize('Text, questions, and voice', '텍스트, 질문, 음성'),
			visible: runtimeSupported,
		},
		{
			id: 'runtime-section-storage',
			label: localize('Storage', '저장소'),
			detail: localize('Persistence backend and trigger', '영속화 백엔드와 트리거'),
			visible: runtimeSupported,
		},
		{
			id: 'runtime-section-summary-batch',
			label: localize('Batch', '배치'),
			detail: localize('Background summary generation', '백그라운드 요약 생성'),
			visible: runtimeSupported,
		},
		{
			id: 'runtime-section-lifecycle',
			label: localize('Lifecycle', '수명주기'),
			detail: localize('TTL and cleanup intervals', 'TTL과 정리 주기'),
			visible: runtimeSupported,
		},
	];
	return items.filter((item) => item.visible);
});

const runtimeQuickBatchScopeLabel = $derived.by(() =>
	runtimeBatchScope === 'all'
		? localize('all sessions', '모든 세션')
		: localize(`${runtimeBatchRecentDays} days`, `${runtimeBatchRecentDays}일`),
);

const runtimeActivityCards = $derived.by((): RuntimeActivityCard[] => {
	const filterLines = (lines: Array<string | null | undefined>): string[] =>
		lines.filter((line): line is string => typeof line === 'string' && line.length > 0);

	return [
		{
			testId: 'runtime-activity-vector',
			title: localize('Vector index', '벡터 인덱스'),
			subtitle: `${runtimeVectorProvider} · ${runtimeVectorModel}`,
			badges: [
				{
					label: toggleWord(runtimeVectorEnabled),
					tone: runtimeVectorEnabled ? 'enabled' : 'disabled',
				},
				{
					label: localize(runtimeVectorIndex?.state ?? 'idle', runtimeVectorIndex?.state === 'running' ? '실행 중' : runtimeVectorIndex?.state === 'failed' ? '실패' : runtimeVectorIndex?.state === 'complete' ? '완료' : '유휴'),
					tone: activityStateTone(runtimeVectorIndex?.state),
				},
			],
			lines: filterLines([
				localize(
					`Provider reachable ${boolWord(runtimeVectorPreflight?.ollama_reachable ?? false)} · model installed ${boolWord(runtimeVectorPreflight?.model_installed ?? false)}`,
					`프로바이더 연결 ${boolWord(runtimeVectorPreflight?.ollama_reachable ?? false)} · 모델 설치 ${boolWord(runtimeVectorPreflight?.model_installed ?? false)}`,
				),
				vectorIndexProgressLabel(runtimeVectorIndex) ?? localize('No rebuild progress recorded yet.', '아직 재구성 진행 기록이 없습니다.'),
				runtimeVectorIndex?.message,
			]),
			timestampLine: localize(
				`started ${formatDate(runtimeVectorIndex?.started_at)} · finished ${formatDate(runtimeVectorIndex?.finished_at)}`,
				`시작 ${formatDate(runtimeVectorIndex?.started_at)} · 종료 ${formatDate(runtimeVectorIndex?.finished_at)}`,
			),
		},
		{
			testId: 'runtime-activity-summary-batch',
			title: localize('Summary batch', '요약 배치'),
			subtitle:
				runtimeBatchExecutionMode === 'on_app_start'
					? localize('auto on app start', '앱 시작 시 자동')
					: localize('manual only', '수동 전용'),
			badges: [
				{
					label: runtimeBatchExecutionMode === 'on_app_start' ? localize('Auto', '자동') : localize('Manual', '수동'),
					tone: runtimeBatchExecutionMode === 'on_app_start' ? 'enabled' : 'disabled',
				},
				{
					label: localize(runtimeSummaryBatchStatus?.state ?? 'idle', runtimeSummaryBatchStatus?.state === 'running' ? '실행 중' : runtimeSummaryBatchStatus?.state === 'failed' ? '실패' : runtimeSummaryBatchStatus?.state === 'complete' ? '완료' : '유휴'),
					tone: activityStateTone(runtimeSummaryBatchStatus?.state),
				},
			],
			lines: filterLines([
				localize(
					`scope ${runtimeBatchScope === 'all' ? 'all sessions' : `${runtimeBatchRecentDays} days`}`,
					`범위 ${runtimeBatchScope === 'all' ? '모든 세션' : `${runtimeBatchRecentDays}일`}`,
				),
				summaryBatchProgressLabel(runtimeSummaryBatchStatus) ?? localize('No batch runs recorded yet.', '아직 배치 실행 기록이 없습니다.'),
				runtimeSummaryBatchStatus?.message,
			]),
			timestampLine: localize(
				`started ${formatDate(runtimeSummaryBatchStatus?.started_at)} · finished ${formatDate(runtimeSummaryBatchStatus?.finished_at)}`,
				`시작 ${formatDate(runtimeSummaryBatchStatus?.started_at)} · 종료 ${formatDate(runtimeSummaryBatchStatus?.finished_at)}`,
			),
		},
		{
			testId: 'runtime-activity-lifecycle',
			title: localize('Lifecycle cleanup', '수명주기 정리'),
			subtitle: localize(
				`${runtimeSessionTtlDays}d session TTL · ${runtimeSummaryTtlDays}d summary TTL`,
				`세션 TTL ${runtimeSessionTtlDays}일 · 요약 TTL ${runtimeSummaryTtlDays}일`,
			),
			badges: [
				{
					label: toggleWord(runtimeLifecycleEnabled),
					tone: runtimeLifecycleEnabled ? 'enabled' : 'disabled',
				},
				{
					label: localize(runtimeLifecycleStatus?.state ?? 'idle', runtimeLifecycleStatus?.state === 'running' ? '실행 중' : runtimeLifecycleStatus?.state === 'failed' ? '실패' : runtimeLifecycleStatus?.state === 'complete' ? '완료' : '유휴'),
					tone: activityStateTone(runtimeLifecycleStatus?.state),
				},
			],
			lines: filterLines([
				localize(
					`interval ${formatIntervalSeconds(runtimeCleanupIntervalSecs)} · next ${lifecycleNextRunLabel()}`,
					`주기 ${formatIntervalSeconds(runtimeCleanupIntervalSecs)} · 다음 ${lifecycleNextRunLabel()}`,
				),
				lifecycleResultLabel(runtimeLifecycleStatus),
				runtimeLifecycleStatus?.message,
			]),
			timestampLine: localize(
				`started ${formatDate(runtimeLifecycleStatus?.started_at)} · finished ${formatDate(runtimeLifecycleStatus?.finished_at)}`,
				`시작 ${formatDate(runtimeLifecycleStatus?.started_at)} · 종료 ${formatDate(runtimeLifecycleStatus?.finished_at)}`,
			),
		},
	];
});

const runtimeQuickSummaryTriggerDetail = $derived.by(() =>
	runtimeTriggerMode === 'on_session_save'
		? localize('runs automatically on new saves', '새 저장 시 자동으로 실행')
		: localize('manual only', '수동 전용'),
);

const runtimeQuickBatchDetail = $derived.by(
	() =>
		localize(
			`scope ${runtimeBatchScope === 'all' ? 'all sessions' : `${runtimeBatchRecentDays} days`}`,
			`범위 ${runtimeBatchScope === 'all' ? '모든 세션' : `${runtimeBatchRecentDays}일`}`,
		),
);

const runtimeQuickBatchStatusDetail = $derived.by(
	() => summaryBatchProgressLabel(runtimeSummaryBatchStatus) ?? localize('No batch runs yet.', '아직 배치 실행이 없습니다.'),
);

const runtimeQuickLifecycleDetail = $derived.by(
	() =>
		localize(
			`${runtimeSessionTtlDays}d session TTL · every ${formatIntervalSeconds(runtimeCleanupIntervalSecs)}`,
			`세션 TTL ${runtimeSessionTtlDays}일 · ${formatIntervalSeconds(runtimeCleanupIntervalSecs)}마다`,
		),
);

const runtimeQuickLifecycleNextDetail = $derived.by(() =>
	localize(`next ${lifecycleNextRunLabel()}`, `다음 ${lifecycleNextRunLabel()}`),
);

const runtimeQuickVectorDetail = $derived.by(() => {
	const base = `${runtimeVectorProvider} · ${runtimeVectorModel}`;
	if (runtimeVectorPreflight && !runtimeVectorPreflight.model_installed) {
		return localize(`${base} · model missing`, `${base} · 모델 없음`);
	}
	return base;
});

const runtimeQuickVectorStatusDetail = $derived.by(
	() =>
		vectorIndexProgressLabel(runtimeVectorIndex) ??
		localize(`index ${runtimeVectorIndex?.state ?? 'idle'}`, `인덱스 ${runtimeVectorIndex?.state === 'running' ? '실행 중' : runtimeVectorIndex?.state === 'failed' ? '실패' : runtimeVectorIndex?.state === 'complete' ? '완료' : '유휴'}`),
);

const runtimeQuickChangeReaderDetail = $derived.by(
	() =>
		localize(
			`text reader · ${runtimeChangeReaderScope} · ${runtimeChangeReaderMaxContextChars.toLocaleString()} chars`,
			`텍스트 리더 · ${runtimeChangeReaderScope} · ${runtimeChangeReaderMaxContextChars.toLocaleString()}자`,
		),
);

const runtimeHelp = {
	defaultSessionView:
		localize(
			'full shows the complete raw session. compressed prioritizes semantic summary + condensed context.',
			'full은 전체 원본 세션을 보여주고, compressed는 시맨틱 요약과 압축된 맥락을 우선합니다.',
		),
	summaryProvider:
		localize(
			'disabled turns off summary generation. ollama uses local HTTP inference. codex_exec/claude_cli run local CLI providers.',
			'disabled는 요약 생성을 끕니다. ollama는 로컬 HTTP 추론을 사용하고, codex_exec/claude_cli는 로컬 CLI 프로바이더를 실행합니다.',
		),
	providerEndpoint: localize('HTTP base URL for ollama or other local model server.', 'ollama 또는 다른 로컬 모델 서버의 HTTP 기본 URL입니다.'),
	providerModel: localize('Model name used by the selected provider.', '선택한 프로바이더가 사용하는 모델 이름입니다.'),
	promptTemplate:
		localize(
			'Template passed to the summary generator. Keep placeholders used by your runtime prompt contract.',
			'요약 생성기에 전달되는 템플릿입니다. 런타임 프롬프트 계약에서 쓰는 placeholder는 유지하세요.',
		),
	responseStyle:
		localize(
			'compact = shortest output, standard = balanced, detailed = richer narrative and context.',
			'compact는 가장 짧은 출력, standard는 균형형, detailed는 더 풍부한 서술과 맥락을 제공합니다.',
		),
	outputShape:
		localize(
			'layered groups by layer, file_list focuses per-file changes, security_first prioritizes auth/security impact.',
			'layered는 레이어별로 묶고, file_list는 파일별 변경에 집중하며, security_first는 인증/보안 영향을 우선합니다.',
		),
	vectorModel: localize('Embedding model name used for vector indexing.', '벡터 인덱싱에 쓰는 임베딩 모델 이름입니다.'),
	vectorEndpoint: localize('Endpoint for local embedding provider (typically Ollama).', '로컬 임베딩 프로바이더용 엔드포인트입니다. 보통 Ollama를 사용합니다.'),
	vectorChunkingMode:
		localize(
			'auto selects chunk size/overlap from session length best-practice rules. manual uses the fixed values below.',
			'auto는 세션 길이에 따른 권장 규칙으로 chunk 크기와 overlap을 정합니다. manual은 아래 고정값을 사용합니다.',
		),
	vectorChunkSize: localize('Number of lines per semantic chunk before embedding.', '임베딩 전 시맨틱 청크당 줄 수입니다.'),
	vectorChunkOverlap: localize('Overlapping lines preserved between adjacent chunks.', '인접 청크 사이에 유지할 겹치는 줄 수입니다.'),
	vectorTopKChunks: localize('Maximum chunk candidates retrieved per query.', '쿼리당 가져올 최대 청크 후보 수입니다.'),
	vectorTopKSessions: localize('Maximum sessions surfaced after chunk ranking.', '청크 랭킹 후 노출할 최대 세션 수입니다.'),
	vectorEnable:
		localize(
			'Turns on semantic retrieval in search and change analysis. Requires model install and index build.',
			'검색과 변경 분석에서 시맨틱 검색을 켭니다. 모델 설치와 인덱스 구축이 필요합니다.',
		),
	changeReaderEnable:
		localize(
			'Turns on the text-based change reader so you can inspect what changed and why across a session.',
			'텍스트 기반 변경 리더를 켜서 세션 전반에서 무엇이 왜 바뀌었는지 살펴볼 수 있게 합니다.',
		),
	changeReaderScope:
		localize(
			'summary_only reads compressed context. full_context expands to broader session context when needed.',
			'summary_only는 압축된 맥락을 읽고, full_context는 필요 시 더 넓은 세션 맥락으로 확장합니다.',
		),
	changeReaderMaxContext: localize('Upper bound of context text loaded for change reading.', '변경 읽기에 불러올 컨텍스트 텍스트의 최대 길이입니다.'),
	changeReaderQa:
		localize(
			'Adds follow-up text questions on top of the selected change reader context when the reader is enabled.',
			'리더가 켜져 있을 때, 선택된 변경 리더 맥락 위에 후속 텍스트 질문을 추가합니다.',
		),
	changeReaderVoiceEnable:
		localize(
			'Reads the current change reader output aloud with TTS. Requires a Voice API key and does not change summaries or questions.',
			'TTS로 현재 변경 리더 출력을 소리 내어 읽습니다. Voice API 키가 필요하며, 요약이나 질문 내용을 바꾸지 않습니다.',
		),
	changeReaderVoiceProvider: localize('Voice provider for TTS playback.', 'TTS 재생에 사용할 음성 프로바이더입니다.'),
	changeReaderVoiceModel: localize('TTS model used when generating speech audio.', '음성 오디오 생성에 사용할 TTS 모델입니다.'),
	changeReaderVoiceName: localize('Voice preset name used by the provider.', '프로바이더가 사용하는 음성 프리셋 이름입니다.'),
	changeReaderVoiceApiKey:
		localize(
			'Write-only API key for voice playback. Required before voice playback can be enabled. Leave empty to keep the current stored key.',
			'음성 재생용 쓰기 전용 API 키입니다. 음성 재생을 켜기 전에 필요합니다. 현재 저장된 키를 유지하려면 비워 두세요.',
		),
	storageTrigger: localize('manual runs only when explicitly requested. on_session_save runs automatically on new saves.', 'manual은 명시적으로 요청할 때만 실행합니다. on_session_save는 새 저장 시 자동 실행됩니다.'),
	storageBackend:
		localize(
			'Where summaries are read from and written to after you click Save Runtime. Switching between hidden_ref and local_db copies existing summaries into the selected backend on save. Switching to none does not migrate or delete existing stored summaries.',
			'런타임 저장을 눌렀을 때 요약을 읽고 쓰는 위치입니다. hidden_ref와 local_db 사이를 바꾸면 저장 시 기존 요약을 선택한 백엔드로 복사합니다. none으로 바꾸면 기존 저장 요약을 마이그레이션하거나 삭제하지 않습니다.',
		),
	batchExecution:
		localize(
			'manual means run only when clicking Run now. on_app_start runs once automatically at desktop startup.',
			'manual은 지금 실행을 눌렀을 때만 동작합니다. on_app_start는 데스크톱 시작 시 한 번 자동 실행됩니다.',
		),
	batchScope:
		localize(
			'recent_days targets only recent sessions. all targets every known session regardless of recency.',
			'recent_days는 최근 세션만 대상으로 하고, all은 시점과 관계없이 알려진 모든 세션을 대상으로 합니다.',
		),
	batchRecentDays: localize('Applies only when scope is recent_days. Minimum is 1 day.', '범위가 recent_days일 때만 적용됩니다. 최소값은 1일입니다.'),
	lifecycleEnable:
		localize(
			'Enables periodic TTL cleanup for session roots and summary artifacts using the rules below.',
			'아래 규칙에 따라 세션 루트와 요약 아티팩트에 대한 주기적 TTL 정리를 활성화합니다.',
		),
	sessionTtl: localize('Sessions older than this threshold become cleanup candidates.', '이 기준보다 오래된 세션이 정리 후보가 됩니다.'),
	summaryTtl: localize('Summary artifacts older than this threshold become cleanup candidates.', '이 기준보다 오래된 요약 아티팩트가 정리 후보가 됩니다.'),
	cleanupInterval: localize('Seconds between periodic lifecycle cleanup runs.', '주기적 수명주기 정리 실행 사이의 초 단위 간격입니다.'),
};

function responsePreview(
	style: DesktopSummaryResponseStyle,
	shape: DesktopSummaryOutputShape,
): string {
	const changesPrefix =
		style === 'compact'
			? localize('Updated session summary pipeline.', '세션 요약 파이프라인을 업데이트했습니다.')
			: style === 'detailed'
				? localize(
					'Refactored the desktop summary pipeline, split provider/prompt/response/storage concerns, and updated hidden-ref persistence semantics.',
					'데스크톱 요약 파이프라인을 리팩터링하고 프로바이더/프롬프트/응답/저장소 관심사를 분리했으며, hidden_ref 영속화 의미를 업데이트했습니다.',
				)
				: localize('Updated desktop summary pipeline with clearer runtime settings.', '더 명확한 런타임 설정으로 데스크톱 요약 파이프라인을 업데이트했습니다.');
	const security =
		shape === 'security_first'
			? localize(
				'Credential paths were isolated and storage policy now defaults to hidden_ref.',
				'자격 증명 경로를 분리했고 저장소 정책 기본값을 hidden_ref로 맞췄습니다.',
			)
			: localize('none detected', '탐지되지 않음');

	const files =
		shape === 'file_list'
			? ['desktop/src-tauri/src/main.rs', 'packages/ui/src/components/SettingsPage.svelte']
			: ['desktop/src-tauri/src/main.rs'];
	const layer =
		shape === 'file_list'
			? localize('application', '애플리케이션')
			: localize('presentation', '프레젠테이션');

	return JSON.stringify(
		{
			changes: changesPrefix,
			auth_security: security,
			layer_file_changes: [
				{
					layer,
					summary:
						style === 'compact'
							? localize('Settings/runtime summary flow updated.', '설정/런타임 요약 흐름을 업데이트했습니다.')
							: localize('Runtime settings and summary persistence behavior were updated.', '런타임 설정과 요약 영속화 동작을 업데이트했습니다.'),
					files,
				},
			],
		},
		null,
		2,
	);
}

let credentialLabel = $state('');
let credentialHost = $state('');
let credentialPathPrefix = $state('');
let credentialHeaderName = $state('Authorization');
let credentialHeaderValue = $state('');

function formatDate(value: string | null | undefined): string {
	if (!value) return '-';
	const parsed = new Date(value);
	if (Number.isNaN(parsed.getTime())) return '-';
	return parsed.toLocaleString(isKorean ? 'ko-KR' : 'en-US');
}

function normalizeError(err: unknown, fallback: string): string {
	if (err instanceof ApiError) return err.message || fallback;
	if (err instanceof Error) return err.message || fallback;
	return fallback;
}

function apiDetailString(
	details: Record<string, unknown> | null | undefined,
	key: string,
): string | null {
	const value = details?.[key];
	return typeof value === 'string' && value.trim() ? value.trim() : null;
}

function apiDetailNumber(
	details: Record<string, unknown> | null | undefined,
	key: string,
): number | null {
	const value = details?.[key];
	return typeof value === 'number' && Number.isFinite(value) ? value : null;
}

function normalizeVectorError(err: unknown, fallback: string): string {
	if (err instanceof ApiError) {
		const message = err.message || fallback;
		const hint = apiDetailString(err.details, 'hint');
		const endpoint = apiDetailString(err.details, 'endpoint');
		const reason = apiDetailString(err.details, 'reason');
		const model = apiDetailString(err.details, 'model');
		const status = apiDetailNumber(err.details, 'status');
		const batchReason = apiDetailString(err.details, 'batch_reason');
		const batchEndpoint = apiDetailString(err.details, 'batch_endpoint');
		const batchStatus = apiDetailNumber(err.details, 'batch_status');
		const lines = [message];
		if (reason) lines.push(localize(`Reason: ${reason}`, `원인: ${reason}`));
		if (status != null) lines.push(`HTTP: ${status}`);
		if (batchReason) lines.push(localize(`Batch reason: ${batchReason}`, `배치 원인: ${batchReason}`));
		if (batchStatus != null) lines.push(localize(`Batch HTTP: ${batchStatus}`, `배치 HTTP: ${batchStatus}`));
		if (hint) lines.push(localize(`Action: ${hint}`, `조치: ${hint}`));
		if (model) lines.push(localize(`Model: ${model}`, `모델: ${model}`));
		if (endpoint) lines.push(localize(`Endpoint: ${endpoint}`, `엔드포인트: ${endpoint}`));
		if (batchEndpoint) lines.push(localize(`Batch endpoint: ${batchEndpoint}`, `배치 엔드포인트: ${batchEndpoint}`));
		return lines.join('\n');
	}
	return normalizeError(err, fallback);
}

function vectorStatusGuidance(): string[] {
	if (!runtimeVectorPreflight) {
		return [localize('Run vector preflight to inspect provider and model readiness.', '프로바이더와 모델 준비 상태를 보려면 벡터 사전 점검을 실행하세요.')];
	}
	const guidance: string[] = [];
	if (!runtimeVectorPreflight.ollama_reachable) {
		guidance.push(localize('Install Ollama: https://ollama.com/download', 'Ollama 설치: https://ollama.com/download'));
		guidance.push(localize('Start provider: run `ollama serve` and retry preflight.', '프로바이더 시작: `ollama serve`를 실행한 뒤 사전 점검을 다시 시도하세요.'));
	}
	if (!runtimeVectorPreflight.model_installed) {
		guidance.push(
			localize(
				`Install model: run \`ollama pull ${runtimeVectorPreflight.model}\` or click "Install model".`,
				`모델 설치: \`ollama pull ${runtimeVectorPreflight.model}\`를 실행하거나 "모델 설치"를 누르세요.`,
			),
		);
	}
	if (runtimeVectorIndex?.state === 'failed') {
		guidance.push(localize('Rebuild index: click "Rebuild index" after fixing provider/model issues.', '인덱스 재구성: 프로바이더/모델 문제를 고친 뒤 "인덱스 재구성"을 누르세요.'));
	}
	if (guidance.length === 0) {
		guidance.push(localize('Vector pipeline is ready.', '벡터 파이프라인이 준비되었습니다.'));
	}
	return guidance;
}

async function loadSettings() {
	loading = true;
	const result = await loadSettingsPageState({
		getApiCapabilities,
		isAuthenticated,
		getSettings,
		listGitCredentials,
	});
	authApiEnabled = result.authApiEnabled;
	authRequired = result.authRequired;
	settings = result.settings;
	error = result.error;
	credentials = result.credentials;
	credentialsLoading = result.credentialsLoading;
	credentialsError = result.credentialsError;
	credentialsSupported = result.credentialsSupported;
	loading = false;
}

async function loadGitCredentials() {
	credentialsLoading = true;
	const result = await loadGitCredentialsState({ listGitCredentials });
	credentials = result.credentials;
	credentialsLoading = result.credentialsLoading;
	credentialsError = result.credentialsError;
	credentialsSupported = result.credentialsSupported;
}

function applyRuntimeSettingsToDraft(settings: DesktopRuntimeSettingsResponse) {
	runtimeSessionDefaultView =
		settings.session_default_view === 'compressed' ? 'compressed' : 'full';
	runtimeProvider = settings.summary.provider.id;
	runtimeProviderTransport = settings.summary.provider.transport;
	runtimeEndpoint = settings.summary.provider.endpoint ?? '';
	runtimeModel = settings.summary.provider.model ?? '';
	runtimeSourceMode = settings.ui_constraints.source_mode_locked
		? settings.ui_constraints.source_mode_locked_value
		: (settings.summary.source_mode ?? 'session_only');
	runtimeResponseStyle = settings.summary.response.style ?? 'standard';
	runtimeOutputShape = settings.summary.response.shape ?? 'layered';
	runtimePromptTemplate = settings.summary.prompt.template ?? '';
	runtimePromptDefaultTemplate = settings.summary.prompt.default_template ?? '';
	runtimeTriggerMode = settings.summary.storage.trigger ?? 'on_session_save';
	runtimeStorageBackend = settings.summary.storage.backend ?? 'hidden_ref';
	runtimeBatchExecutionMode = settings.summary.batch.execution_mode ?? 'on_app_start';
	runtimeBatchScope = settings.summary.batch.scope ?? 'recent_days';
	runtimeBatchRecentDays = settings.summary.batch.recent_days ?? 30;
	runtimeVectorEnabled = settings.vector_search.enabled ?? false;
	runtimeVectorProvider = settings.vector_search.provider ?? 'ollama';
	runtimeVectorModel = settings.vector_search.model ?? 'bge-m3';
	runtimeVectorEndpoint = settings.vector_search.endpoint ?? 'http://127.0.0.1:11434';
	runtimeVectorGranularity = settings.vector_search.granularity ?? 'event_line_chunk';
	runtimeVectorChunkingMode = settings.vector_search.chunking_mode ?? 'auto';
	runtimeVectorChunkSizeLines = settings.vector_search.chunk_size_lines ?? 12;
	runtimeVectorChunkOverlapLines = settings.vector_search.chunk_overlap_lines ?? 3;
	runtimeVectorTopKChunks = settings.vector_search.top_k_chunks ?? 30;
	runtimeVectorTopKSessions = settings.vector_search.top_k_sessions ?? 20;
	runtimeChangeReaderEnabled = settings.change_reader?.enabled ?? false;
	runtimeChangeReaderScope = settings.change_reader?.scope ?? 'summary_only';
	runtimeChangeReaderQaEnabled = settings.change_reader?.qa_enabled ?? true;
	runtimeChangeReaderMaxContextChars = settings.change_reader?.max_context_chars ?? 12000;
	runtimeChangeReaderVoiceEnabled = settings.change_reader?.voice?.enabled ?? false;
	runtimeChangeReaderVoiceProvider = settings.change_reader?.voice?.provider ?? 'openai';
	runtimeChangeReaderVoiceModel = settings.change_reader?.voice?.model ?? 'gpt-4o-mini-tts';
	runtimeChangeReaderVoiceName = settings.change_reader?.voice?.voice ?? 'alloy';
	runtimeChangeReaderVoiceApiKeyConfigured =
		settings.change_reader?.voice?.api_key_configured ?? false;
	runtimeChangeReaderVoiceApiKey = '';
	runtimeLifecycleEnabled = settings.lifecycle?.enabled ?? true;
	runtimeSessionTtlDays = settings.lifecycle?.session_ttl_days ?? 30;
	runtimeSummaryTtlDays = settings.lifecycle?.summary_ttl_days ?? 30;
	runtimeCleanupIntervalSecs = settings.lifecycle?.cleanup_interval_secs ?? 3600;
}

async function loadRuntimeSettings() {
	runtimeLoading = true;
	const result = await loadRuntimeSettingsState({
		getRuntimeSettings,
		getLifecycleCleanupStatus,
		getSummaryBatchStatus,
		vectorPreflight,
		vectorIndexStatus,
	});
	runtimeSettings = result.runtimeSettings;
	runtimeSupported = result.runtimeSupported;
	runtimeError = result.runtimeError;
	runtimeVectorError = result.runtimeVectorError;
	runtimeLifecycleStatus = result.runtimeLifecycleStatus;
	runtimeSummaryBatchStatus = result.runtimeSummaryBatchStatus;
	runtimeVectorPreflight = result.runtimeVectorPreflight;
	runtimeVectorIndex = result.runtimeVectorIndex;
	runtimeVectorInstalling = result.runtimeVectorInstalling;
	runtimeVectorReindexing = result.runtimeVectorReindexing;
	runtimeSummaryBatchRunning = result.runtimeSummaryBatchRunning;
	if (runtimeSettings) {
		applyRuntimeSettingsToDraft(runtimeSettings);
	}
	runtimeLoading = false;
}

async function refreshLifecycleCleanupStatus(surfaceError: boolean = true): Promise<boolean> {
	try {
		runtimeLifecycleStatus = await getLifecycleCleanupStatus();
		return true;
	} catch (err) {
		runtimeLifecycleStatus = null;
		if (surfaceError) {
			runtimeError = normalizeError(err, localize('Failed to fetch lifecycle cleanup status', '수명주기 정리 상태를 가져오지 못했습니다'));
		}
		return false;
	}
}

function buildRuntimeSummaryPayload() {
	return {
		provider: {
			id: runtimeProvider,
			endpoint: runtimeEndpoint,
			model: runtimeModel,
		},
		prompt: {
			template: runtimePromptTemplate,
		},
		response: {
			style: runtimeResponseStyle,
			shape: runtimeOutputShape,
		},
		storage: {
			trigger: runtimeTriggerMode,
			backend: runtimeStorageBackend,
		},
		source_mode: runtimeSourceMode,
		batch: {
			execution_mode: runtimeBatchExecutionMode,
			scope: runtimeBatchScope,
			recent_days: runtimeBatchRecentDays,
		},
	};
}

function buildRuntimeVectorPayload() {
	return {
		enabled: runtimeVectorEnabled,
		provider: runtimeVectorProvider,
		model: runtimeVectorModel,
		endpoint: runtimeVectorEndpoint,
		granularity: runtimeVectorGranularity,
		chunking_mode: runtimeVectorChunkingMode,
		chunk_size_lines: runtimeVectorChunkSizeLines,
		chunk_overlap_lines: runtimeVectorChunkOverlapLines,
		top_k_chunks: runtimeVectorTopKChunks,
		top_k_sessions: runtimeVectorTopKSessions,
	};
}

function buildRuntimeChangeReaderPayload() {
	return {
		enabled: runtimeChangeReaderEnabled,
		scope: runtimeChangeReaderScope,
		qa_enabled: runtimeChangeReaderQaEnabled,
		max_context_chars: runtimeChangeReaderMaxContextChars,
		voice: {
			enabled: runtimeChangeReaderVoiceEnabled,
			provider: runtimeChangeReaderVoiceProvider,
			model: runtimeChangeReaderVoiceModel,
			voice: runtimeChangeReaderVoiceName,
			api_key: runtimeChangeReaderVoiceApiKey.trim() || null,
		},
	};
}

function buildRuntimeLifecyclePayload() {
	return {
		enabled: runtimeLifecycleEnabled,
		session_ttl_days: runtimeSessionTtlDays,
		summary_ttl_days: runtimeSummaryTtlDays,
		cleanup_interval_secs: runtimeCleanupIntervalSecs,
	};
}

function buildRuntimeDraftSnapshot(): RuntimeDraftSnapshot {
	return {
		session_default_view: runtimeSessionDefaultView,
		summary: buildRuntimeSummaryPayload(),
		vector_search: buildRuntimeVectorPayload(),
		change_reader: {
			...buildRuntimeChangeReaderPayload(),
			voice: {
				enabled: runtimeChangeReaderVoiceEnabled,
				provider: runtimeChangeReaderVoiceProvider,
				model: runtimeChangeReaderVoiceModel,
				voice: runtimeChangeReaderVoiceName,
				api_key_pending: runtimeChangeReaderVoiceApiKey.trim().length > 0,
			},
		},
		lifecycle: buildRuntimeLifecyclePayload(),
	};
}

function buildPersistedRuntimeSnapshot(
	settings: DesktopRuntimeSettingsResponse,
): RuntimeDraftSnapshot {
	return {
		session_default_view:
			settings.session_default_view === 'compressed' ? 'compressed' : 'full',
		summary: {
			provider: {
				id: settings.summary.provider.id,
				endpoint: settings.summary.provider.endpoint ?? '',
				model: settings.summary.provider.model ?? '',
			},
			prompt: {
				template: settings.summary.prompt.template ?? '',
			},
			response: {
				style: settings.summary.response.style ?? 'standard',
				shape: settings.summary.response.shape ?? 'layered',
			},
			storage: {
				trigger: settings.summary.storage.trigger ?? 'on_session_save',
				backend: settings.summary.storage.backend ?? 'hidden_ref',
			},
			source_mode: settings.summary.source_mode ?? 'session_only',
			batch: {
				execution_mode: settings.summary.batch.execution_mode ?? 'on_app_start',
				scope: settings.summary.batch.scope ?? 'recent_days',
				recent_days: settings.summary.batch.recent_days ?? 30,
			},
		},
		vector_search: {
			enabled: settings.vector_search.enabled ?? false,
			provider: settings.vector_search.provider ?? 'ollama',
			model: settings.vector_search.model ?? 'bge-m3',
			endpoint: settings.vector_search.endpoint ?? 'http://127.0.0.1:11434',
			granularity: settings.vector_search.granularity ?? 'event_line_chunk',
			chunking_mode: settings.vector_search.chunking_mode ?? 'auto',
			chunk_size_lines: settings.vector_search.chunk_size_lines ?? 12,
			chunk_overlap_lines: settings.vector_search.chunk_overlap_lines ?? 3,
			top_k_chunks: settings.vector_search.top_k_chunks ?? 30,
			top_k_sessions: settings.vector_search.top_k_sessions ?? 20,
		},
		change_reader: {
			enabled: settings.change_reader?.enabled ?? false,
			scope: settings.change_reader?.scope ?? 'summary_only',
			qa_enabled: settings.change_reader?.qa_enabled ?? true,
			max_context_chars: settings.change_reader?.max_context_chars ?? 12000,
			voice: {
				enabled: settings.change_reader?.voice?.enabled ?? false,
				provider: settings.change_reader?.voice?.provider ?? 'openai',
				model: settings.change_reader?.voice?.model ?? 'gpt-4o-mini-tts',
				voice: settings.change_reader?.voice?.voice ?? 'alloy',
				api_key_pending: false,
			},
		},
		lifecycle: {
			enabled: settings.lifecycle?.enabled ?? true,
			session_ttl_days: settings.lifecycle?.session_ttl_days ?? 30,
			summary_ttl_days: settings.lifecycle?.summary_ttl_days ?? 30,
			cleanup_interval_secs: settings.lifecycle?.cleanup_interval_secs ?? 3600,
		},
	};
}

function runtimeSnapshotKey(snapshot: RuntimeDraftSnapshot): string {
	return JSON.stringify(snapshot);
}

const runtimeDraftDirty = $derived.by(() => {
	if (!runtimeSettings) return false;
	return (
		runtimeSnapshotKey(buildRuntimeDraftSnapshot()) !==
		runtimeSnapshotKey(buildPersistedRuntimeSnapshot(runtimeSettings))
	);
});

const runtimePersistStatus = $derived.by(() => {
	if (runtimeLoading) {
		return {
			title: localize('Loading runtime config', '런타임 설정 불러오는 중'),
			detail: localize('Fetching the current persisted desktop runtime settings.', '현재 저장된 데스크톱 런타임 설정을 가져오고 있습니다.'),
		};
	}
	if (runtimeDraftDirty) {
		return {
			title: localize('Unsaved runtime changes', '저장되지 않은 런타임 변경 사항'),
			detail:
				localize(
					'Checkbox, select, and input edits are drafts until you click Save Runtime. Reopening Settings reloads the last persisted values.',
					'체크박스, 선택, 입력 변경은 런타임 저장을 누르기 전까지 초안입니다. 설정을 다시 열면 마지막으로 저장된 값이 다시 로드됩니다.',
				),
		};
	}
	return {
		title: localize('Runtime config is persisted', '런타임 설정이 저장된 상태입니다'),
		detail:
			localize(
				'Current values match the saved desktop runtime config. New edits stay local to this page until you save them.',
				'현재 값은 저장된 데스크톱 런타임 설정과 일치합니다. 새 수정 사항은 저장하기 전까지 이 페이지에만 반영됩니다.',
			),
	};
});

function handleRuntimeProviderChange() {
	runtimeProviderTransport = currentRuntimeProviderTransport();
	if (runtimeProviderTransport !== 'http') {
		runtimeEndpoint = '';
	}
}

function handleResetPromptTemplate() {
	runtimePromptTemplate = runtimePromptDefaultTemplate;
}

function handleResetRuntimeDraft() {
	if (!runtimeSettings) return;
	applyRuntimeSettingsToDraft(runtimeSettings);
	runtimeError = null;
	runtimeDetectMessage = localize('Discarded unsaved runtime edits.', '저장되지 않은 런타임 변경을 버렸습니다.');
}

async function handleSaveRuntimeSettings() {
	runtimeSaving = true;
	runtimeError = null;
	runtimeDetectMessage = null;
	try {
		const updated = await updateRuntimeSettings({
			session_default_view: runtimeSessionDefaultView,
			summary: buildRuntimeSummaryPayload(),
			vector_search: buildRuntimeVectorPayload(),
			change_reader: buildRuntimeChangeReaderPayload(),
			lifecycle: buildRuntimeLifecyclePayload(),
		});
		runtimeSettings = updated;
		applyRuntimeSettingsToDraft(updated);
		await refreshLifecycleCleanupStatus(false);
		await refreshSummaryBatchStatus();
		runtimeDetectMessage = localize(
			'Runtime settings saved and will persist when you reopen Settings.',
			'런타임 설정을 저장했습니다. 설정을 다시 열어도 유지됩니다.',
		);
	} catch (err) {
		runtimeError = normalizeError(err, localize('Failed to save runtime settings', '런타임 설정 저장에 실패했습니다'));
	} finally {
		runtimeSaving = false;
	}
}

async function handleDetectRuntimeProvider() {
	runtimeDetecting = true;
	runtimeDetectMessage = null;
	runtimeError = null;
	try {
		const detected: DesktopSummaryProviderDetectResponse = await detectSummaryProvider();
		if (!detected.detected || !detected.provider) {
			runtimeDetectMessage = localize('No local provider detected.', '로컬 프로바이더를 찾지 못했습니다.');
			return;
		}
		runtimeProvider = detected.provider;
		runtimeProviderTransport =
			detected.transport ?? providerTransportForId(detected.provider ?? 'disabled');
		if (detected.model != null) runtimeModel = detected.model;
		if (detected.endpoint != null) runtimeEndpoint = detected.endpoint;
		const updated = await updateRuntimeSettings({
			summary: buildRuntimeSummaryPayload(),
		});
		runtimeSettings = updated;
		applyRuntimeSettingsToDraft(updated);
		runtimeDetectMessage = localize(
			`Detected and applied provider: ${detected.provider}`,
			`감지 후 적용한 프로바이더: ${detected.provider}`,
		);
	} catch (err) {
		runtimeError = normalizeError(err, localize('Failed to detect/apply local provider', '로컬 프로바이더 감지/적용에 실패했습니다'));
	} finally {
		runtimeDetecting = false;
	}
}

async function refreshSummaryBatchStatus() {
	try {
		runtimeSummaryBatchStatus = await getSummaryBatchStatus();
		runtimeSummaryBatchRunning = isSummaryBatchRunning(runtimeSummaryBatchStatus);
	} catch (err) {
		runtimeSummaryBatchStatus = null;
		runtimeSummaryBatchRunning = false;
		runtimeError = normalizeError(err, localize('Failed to fetch summary batch status', '요약 배치 상태를 가져오지 못했습니다'));
	}
}

async function handleRunSummaryBatchNow() {
	runtimeSummaryBatchRunning = true;
	runtimeError = null;
	try {
		runtimeSummaryBatchStatus = await runSummaryBatch();
		runtimeSummaryBatchRunning = isSummaryBatchRunning(runtimeSummaryBatchStatus);
	} catch (err) {
		runtimeSummaryBatchRunning = false;
		runtimeError = normalizeError(err, localize('Failed to run summary batch', '요약 배치 실행에 실패했습니다'));
	}
}

async function refreshVectorPreflight(): Promise<boolean> {
	try {
		runtimeVectorPreflight = await vectorPreflight();
		runtimeVectorInstalling = isVectorInstallRunning(runtimeVectorPreflight);
		runtimeVectorError = null;
		if (runtimeVectorPreflight.model_installed && runtimeVectorEnabled) {
			runtimeDetectMessage = localize('Vector model is ready.', '벡터 모델이 준비되었습니다.');
		}
		return true;
	} catch (err) {
		runtimeVectorPreflight = null;
		runtimeVectorError = normalizeVectorError(err, localize('Failed to fetch vector model status', '벡터 모델 상태를 가져오지 못했습니다'));
		return false;
	}
}

async function refreshVectorIndexStatus(): Promise<boolean> {
	try {
		runtimeVectorIndex = await vectorIndexStatus();
		runtimeVectorReindexing = isVectorIndexRunning(runtimeVectorIndex);
		return true;
	} catch (err) {
		runtimeVectorIndex = null;
		runtimeVectorError = normalizeVectorError(err, localize('Failed to fetch vector index status', '벡터 인덱스 상태를 가져오지 못했습니다'));
		return false;
	}
}

async function handleVectorInstallModel() {
	runtimeVectorInstalling = true;
	runtimeVectorError = null;
	try {
		const status = await vectorInstallModel(runtimeVectorModel);
		if (status.state === 'failed') {
			runtimeVectorError = status.message ?? localize('Model installation failed.', '모델 설치에 실패했습니다.');
			runtimeVectorInstalling = false;
			return;
		}
		runtimeVectorInstalling = status.state === 'installing';
		await refreshVectorPreflight();
	} catch (err) {
		runtimeVectorInstalling = false;
		runtimeVectorError = normalizeVectorError(err, localize('Failed to install vector model', '벡터 모델 설치에 실패했습니다'));
	}
}

async function handleVectorReindex() {
	const preflightOk = await refreshVectorPreflight();
	if (!preflightOk || !runtimeVectorPreflight) return;
	if (!runtimeVectorPreflight.ollama_reachable) {
		runtimeVectorError =
			runtimeVectorPreflight.message ??
			localize('Ollama is not reachable. Start it with `ollama serve`.', 'Ollama에 연결할 수 없습니다. `ollama serve`로 시작하세요.');
		return;
	}
	if (!runtimeVectorPreflight.model_installed) {
		runtimeVectorError =
			runtimeVectorPreflight.message ??
			localize(
				`Model ${runtimeVectorPreflight.model} is not installed. Install model first.`,
				`모델 ${runtimeVectorPreflight.model}이 설치되지 않았습니다. 먼저 모델을 설치하세요.`,
			);
		return;
	}

	runtimeVectorReindexing = true;
	runtimeVectorError = null;
	try {
		runtimeVectorIndex = await vectorIndexRebuild();
		runtimeVectorReindexing = isVectorIndexRunning(runtimeVectorIndex);
		if (runtimeVectorIndex?.state === 'failed') {
			runtimeVectorError = runtimeVectorIndex.message ?? localize('Vector index rebuild failed.', '벡터 인덱스 재구성에 실패했습니다.');
		}
	} catch (err) {
		runtimeVectorReindexing = false;
		runtimeVectorError = normalizeVectorError(err, localize('Failed to start vector index rebuild', '벡터 인덱스 재구성을 시작하지 못했습니다'));
	}
}

async function handleIssueApiKey() {
	issuing = true;
	error = null;
	copyMessage = null;
	try {
		const response = await issueApiKey();
		issuedApiKey = response.api_key;
	} catch (err) {
		error = normalizeError(err, localize('Failed to issue API key', 'API 키 발급에 실패했습니다'));
	} finally {
		issuing = false;
	}
}

async function copyApiKey() {
	const result = await copyTextSurface(
		{
			writeText: async (text) => {
				if (typeof navigator === 'undefined' || !navigator.clipboard?.writeText) {
					throw new Error('clipboard unavailable');
				}
				await navigator.clipboard.writeText(text);
			},
		},
		issuedApiKey,
	);
	copyMessage =
		result === 'Copied'
			? localize('Copied', '복사했습니다')
			: localize('Copy failed', '복사에 실패했습니다');
}

function currentBackgroundPollState() {
	return {
		runtimeSupported,
		runtimeLifecycleEnabled,
		runtimeVectorInstalling,
		runtimeVectorPreflight,
		runtimeVectorReindexing,
		runtimeVectorIndex,
		runtimeSummaryBatchRunning,
		runtimeSummaryBatchStatus,
		runtimeLifecycleStatus,
	};
}

async function handleCreateCredential() {
	creatingCredential = true;
	credentialsError = null;
	try {
		await createGitCredential({
			label: credentialLabel,
			host: credentialHost,
			path_prefix: credentialPathPrefix.trim() ? credentialPathPrefix.trim() : null,
			header_name: credentialHeaderName,
			header_value: credentialHeaderValue,
		});
		credentialLabel = '';
		credentialHost = '';
		credentialPathPrefix = '';
		credentialHeaderName = 'Authorization';
		credentialHeaderValue = '';
		await loadGitCredentials();
	} catch (err) {
		credentialsError = normalizeError(err, localize('Failed to save git credential', 'Git 자격 증명 저장에 실패했습니다'));
	} finally {
		creatingCredential = false;
	}
}

async function handleDeleteCredential(id: string) {
	deletingCredentialId = id;
	credentialsError = null;
	try {
		await deleteGitCredential(id);
		await loadGitCredentials();
	} catch (err) {
		credentialsError = normalizeError(err, localize('Failed to delete git credential', 'Git 자격 증명 삭제에 실패했습니다'));
	} finally {
		deletingCredentialId = null;
	}
}

$effect(() => {
	if (runtimeVectorEnabled && runtimeVectorPreflight && !runtimeVectorPreflight.model_installed) {
		runtimeVectorEnabled = false;
	}
});

$effect(() => {
	const initialDelay = nextSettingsBackgroundPollDelay(
		currentBackgroundPollState(),
		BACKGROUND_JOB_POLL_INTERVAL_MS,
		BACKGROUND_STATUS_POLL_INTERVAL_MS,
	);
	if (initialDelay == null) {
		return;
	}

	let cancelled = false;
	let timer: number | undefined;

	const poll = async () => {
		if (cancelled) return;
		if (runtimeVectorInstalling || isVectorInstallRunning(runtimeVectorPreflight)) {
			await refreshVectorPreflight();
		}
		if (runtimeVectorReindexing || isVectorIndexRunning(runtimeVectorIndex)) {
			await refreshVectorIndexStatus();
		}
		if (runtimeSummaryBatchRunning || isSummaryBatchRunning(runtimeSummaryBatchStatus)) {
			await refreshSummaryBatchStatus();
		}
		if (runtimeLifecycleEnabled || isLifecycleCleanupRunning(runtimeLifecycleStatus)) {
			await refreshLifecycleCleanupStatus(false);
		}
		if (cancelled) return;
		const nextDelay = nextSettingsBackgroundPollDelay(
			currentBackgroundPollState(),
			BACKGROUND_JOB_POLL_INTERVAL_MS,
			BACKGROUND_STATUS_POLL_INTERVAL_MS,
		);
		if (nextDelay == null) return;
		timer = window.setTimeout(poll, nextDelay);
	};

	timer = window.setTimeout(poll, initialDelay);

	return () => {
		cancelled = true;
		if (timer != null) {
			window.clearTimeout(timer);
		}
	};
});

$effect(() => {
	loadSettings();
	loadRuntimeSettings();
});

$effect(() => {
	const visibleIds = settingsNavItems.map((item) => item.id);
	if (visibleIds.length === 0) return;
	if (!visibleIds.includes(activeSettingsSectionId)) {
		activeSettingsSectionId = visibleIds[0];
	}
});
</script>

<svelte:head>
	<title>{localize('Settings - opensession.io', '설정 - opensession.io')}</title>
</svelte:head>

<div data-testid="settings-page" class="mx-auto w-full max-w-7xl pb-10">
	<div class="grid gap-4 xl:grid-cols-[14rem_minmax(0,1fr)] xl:items-start">
		<SettingsSectionNav
			items={settingsNavItems}
			activeId={activeSettingsSectionId}
			onSelect={setActiveSettingsSection}
		/>

		<div class="space-y-4">
	<header id="settings-section-overview" class="scroll-mt-24 border border-border bg-bg-secondary px-4 py-3">
		<p class="text-[11px] uppercase tracking-[0.12em] text-text-muted">{localize('Account', '계정')}</p>
		<h1 class="mt-1 text-3xl font-semibold tracking-tight text-text-primary">{localize('Settings', '설정')}</h1>
		<p class="mt-1 text-sm text-text-secondary">{localize('Personal profile and API access controls.', '개인 프로필과 API 접근 제어를 관리합니다.')}</p>
	</header>

	<LanguageSettingsPanel />

		{#if loading}
			<div class="border border-border bg-bg-secondary px-4 py-8 text-center text-sm text-text-muted">{localize('Loading...', '불러오는 중...')}</div>
		{:else if authApiEnabled && authRequired}
			<section
				id="settings-section-profile"
				data-testid="settings-require-auth"
				class="scroll-mt-24 border border-border bg-bg-secondary px-4 py-6 text-sm text-text-secondary xl:max-w-3xl"
			>
			<p class="text-text-primary">{localize('Sign in is required to view personal settings.', '개인 설정을 보려면 로그인해야 합니다.')}</p>
			<div class="mt-4">
				<button
					type="button"
					onclick={() => onNavigate('/login')}
					class="bg-accent px-3 py-2 text-xs font-semibold text-white hover:bg-accent/85"
				>
					{localize('Go to login', '로그인으로 이동')}
				</button>
			</div>
		</section>
	{:else if authApiEnabled}
		<section id="settings-section-profile" class="scroll-mt-24 border border-border bg-bg-secondary p-4 xl:max-w-3xl">
			<h2 class="text-sm font-semibold text-text-primary">{localize('Profile', '프로필')}</h2>
			{#if settings}
				<dl class="mt-3 grid gap-2 text-xs text-text-secondary sm:grid-cols-[10rem_1fr]">
					<dt>{localize('User ID', '사용자 ID')}</dt>
					<dd class="font-mono text-text-primary">{settings.user_id}</dd>
					<dt>{localize('Nickname', '닉네임')}</dt>
					<dd class="text-text-primary">{settings.nickname}</dd>
					<dt>{localize('Email', '이메일')}</dt>
					<dd class="text-text-primary">{settings.email ?? localize('not linked', '연결되지 않음')}</dd>
					<dt>{localize('Joined', '가입일')}</dt>
					<dd class="text-text-primary">{formatDate(settings.created_at)}</dd>
					<dt>{localize('Linked OAuth', '연결된 OAuth')}</dt>
					<dd class="text-text-primary">
						{#if settings.oauth_providers.length === 0}
							{localize('none', '없음')}
						{:else}
							{settings.oauth_providers.map((provider) => provider.display_name).join(', ')}
						{/if}
					</dd>
				</dl>
			{:else}
				<p class="mt-2 text-xs text-text-muted">{localize('No profile data available.', '프로필 데이터가 없습니다.')}</p>
			{/if}
		</section>

		<section id="settings-section-api-key" class="scroll-mt-24 border border-border bg-bg-secondary p-4 xl:max-w-3xl">
			<div class="flex flex-wrap items-center justify-between gap-3">
				<div>
					<h2 class="text-sm font-semibold text-text-primary">{localize('Personal API Key', '개인 API 키')}</h2>
					<p class="mt-1 text-xs text-text-secondary">
						{localize(
							'Issue a new key for CLI and automation access. Existing active key moves to grace mode.',
							'CLI와 자동화 접근용 새 키를 발급합니다. 기존 활성 키는 유예 상태로 전환됩니다.',
						)}
					</p>
				</div>
				<button
					type="button"
					data-testid="issue-api-key-button"
					onclick={handleIssueApiKey}
					disabled={issuing}
					class="bg-accent px-3 py-2 text-xs font-semibold text-white hover:bg-accent/85 disabled:opacity-60"
				>
					{issuing
						? localize('Issuing...', '발급 중...')
						: issuedApiKey
							? localize('Regenerate key', '키 재발급')
							: localize('Issue key', '키 발급')}
				</button>
			</div>

			{#if issuedApiKey}
				<div class="mt-4 border border-border/80 bg-bg-primary p-3">
					<p class="mb-2 text-xs text-text-muted">{localize('Shown once. Save this key now.', '한 번만 표시됩니다. 지금 이 키를 저장하세요.')}</p>
					<code data-testid="issued-api-key" class="block break-all font-mono text-xs text-text-primary">
						{issuedApiKey}
					</code>
					<div class="mt-3 flex items-center gap-2">
						<button
							type="button"
							data-testid="copy-api-key"
							onclick={copyApiKey}
							class="border border-border px-2 py-1 text-xs text-text-secondary hover:text-text-primary"
						>
							{localize('Copy', '복사')}
						</button>
						{#if copyMessage}
							<span class="text-xs text-text-muted">{copyMessage}</span>
						{/if}
					</div>
				</div>
			{/if}
		</section>

		<section
			id="settings-section-git-credentials"
			class="scroll-mt-24 border border-border bg-bg-secondary p-4 xl:max-w-3xl"
			data-testid="git-credential-settings"
		>
			<div class="space-y-1">
				<h2 class="text-sm font-semibold text-text-primary">{localize('Private Git Credentials', '비공개 Git 자격 증명')}</h2>
				<p class="text-xs text-text-secondary">
					{localize(
						'Preferred: connect GitHub/GitLab OAuth. Manual credentials are used for private self-managed or generic git remotes.',
						'권장: GitHub/GitLab OAuth를 연결하세요. 수동 자격 증명은 비공개 self-managed 또는 일반 git remote에 사용됩니다.',
					)}
				</p>
			</div>

			{#if !credentialsSupported}
				<p class="mt-3 text-xs text-text-muted">
					{localize('This deployment does not expose credential management endpoints.', '이 배포는 자격 증명 관리 엔드포인트를 제공하지 않습니다.')}
				</p>
			{:else}
				<div class="mt-4 space-y-3">
					<div class="grid gap-2 sm:grid-cols-2">
						<input
							data-testid="git-credential-label"
							type="text"
							placeholder={localize('Label', '라벨')}
							bind:value={credentialLabel}
							class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
						/>
						<input
							data-testid="git-credential-host"
							type="text"
							placeholder={localize('Host (e.g. gitlab.internal.example.com)', '호스트 (예: gitlab.internal.example.com)')}
							bind:value={credentialHost}
							class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
						/>
						<input
							data-testid="git-credential-path-prefix"
							type="text"
							placeholder={localize('Path prefix (optional, e.g. group/subgroup)', '경로 접두사 (선택, 예: group/subgroup)')}
							bind:value={credentialPathPrefix}
							class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
						/>
						<input
							data-testid="git-credential-header-name"
							type="text"
							placeholder={localize('Header name', '헤더 이름')}
							bind:value={credentialHeaderName}
							class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
						/>
						<input
							data-testid="git-credential-header-value"
							type="password"
							placeholder={localize('Header value (secret)', '헤더 값 (비밀값)')}
							bind:value={credentialHeaderValue}
							class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary sm:col-span-2"
						/>
					</div>
					<div class="flex justify-end">
						<button
							type="button"
							data-testid="git-credential-save"
							onclick={handleCreateCredential}
							disabled={creatingCredential}
							class="bg-accent px-3 py-2 text-xs font-semibold text-white hover:bg-accent/85 disabled:opacity-60"
						>
							{creatingCredential ? localize('Saving...', '저장 중...') : localize('Save credential', '자격 증명 저장')}
						</button>
					</div>
				</div>

				<div class="mt-4 border border-border/70">
					<div class="grid grid-cols-[1.1fr_1fr_1fr_auto] gap-2 border-b border-border bg-bg-primary px-3 py-2 text-[11px] uppercase tracking-[0.08em] text-text-muted">
						<span>{localize('Label', '라벨')}</span>
						<span>{localize('Host', '호스트')}</span>
						<span>{localize('Path Prefix', '경로 접두사')}</span>
						<span>{localize('Action', '동작')}</span>
					</div>
					{#if credentialsLoading}
						<div class="px-3 py-3 text-xs text-text-muted">{localize('Loading credentials...', '자격 증명을 불러오는 중...')}</div>
					{:else if credentials.length === 0}
						<div class="px-3 py-3 text-xs text-text-muted">{localize('No manual credentials registered.', '등록된 수동 자격 증명이 없습니다.')}</div>
					{:else}
						{#each credentials as credential}
							<div class="grid grid-cols-[1.1fr_1fr_1fr_auto] items-center gap-2 border-b border-border/60 px-3 py-2 text-xs">
								<div class="text-text-primary">{credential.label}</div>
								<div class="font-mono text-[11px] text-text-secondary">{credential.host}</div>
								<div class="font-mono text-[11px] text-text-secondary">{credential.path_prefix || '*'}</div>
								<button
									type="button"
									data-testid={'git-credential-delete-' + credential.id}
									disabled={deletingCredentialId === credential.id}
									onclick={() => handleDeleteCredential(credential.id)}
									class="border border-border px-2 py-1 text-[11px] text-text-secondary hover:text-text-primary disabled:opacity-60"
								>
									{deletingCredentialId === credential.id ? localize('Deleting...', '삭제 중...') : localize('Delete', '삭제')}
								</button>
							</div>
						{/each}
					{/if}
				</div>
				<p class="mt-2 text-[11px] text-text-muted">
					{localize(
						'Secrets are never shown again after save. Stored values are encrypted at rest.',
						'비밀값은 저장 후 다시 표시되지 않습니다. 저장된 값은 at-rest 암호화됩니다.',
					)}
				</p>
			{/if}
		</section>

		{#if error}
			<p class="text-xs text-error">{error}</p>
		{/if}
	{#if credentialsError}
		<p class="text-xs text-error">{credentialsError}</p>
	{/if}
	{/if}

	<section
		id="settings-section-runtime"
		class="scroll-mt-24 border border-border bg-bg-secondary p-4"
		data-testid="runtime-summary-settings"
	>
		<div class="flex flex-wrap items-center justify-between gap-3">
			<div>
				<h2 class="text-sm font-semibold text-text-primary">{localize('Runtime Summary (Desktop)', '런타임 요약 (데스크톱)')}</h2>
				<p class="mt-1 text-xs text-text-secondary">
					{localize(
						'Provider, prompt, response, and storage settings for desktop local runtime.',
						'데스크톱 로컬 런타임의 프로바이더, 프롬프트, 응답, 저장소 설정입니다.',
					)}
				</p>
			</div>
			<div class="flex items-center gap-2">
					<button
						type="button"
						data-testid="runtime-detect-provider"
						onclick={handleDetectRuntimeProvider}
						disabled={!runtimeSupported || runtimeDetecting || runtimeSaving || runtimeLoading}
						class="inline-flex h-9 items-center border border-border px-3 text-xs font-semibold text-text-secondary hover:text-text-primary disabled:opacity-60"
					>
						{runtimeDetecting ? localize('Detecting...', '감지 중...') : localize('Detect Provider', '프로바이더 감지')}
					</button>
						<button
							type="button"
							data-testid="runtime-save"
							onclick={handleSaveRuntimeSettings}
							disabled={!runtimeSupported || runtimeSaving || runtimeLoading}
							class="inline-flex h-9 items-center border border-transparent bg-accent px-3 text-xs font-semibold text-white hover:bg-accent/85 disabled:opacity-60"
						>
							{runtimeSaveLabel()}
						</button>
			</div>
		</div>

		{#if runtimeLoading}
			<p class="mt-3 text-xs text-text-muted">{localize('Loading runtime settings...', '런타임 설정을 불러오는 중...')}</p>
		{:else if !runtimeSupported}
			<p class="mt-3 text-xs text-text-muted">
				{localize(
					'Runtime settings are not available in this environment (desktop IPC required).',
					'이 환경에서는 런타임 설정을 사용할 수 없습니다. (데스크톱 IPC 필요)',
				)}
			</p>
		{:else}
			<div class="mt-4 grid gap-4 xl:grid-cols-[minmax(0,1fr)_18rem] xl:items-start">
				<div class="order-last space-y-4 xl:order-first">
				<div
					class={`rounded border px-3 py-3 text-xs ${
						runtimeDraftDirty
							? 'border-accent/40 bg-accent/5 text-text-primary'
							: 'border-border/60 bg-bg-primary text-text-secondary'
					}`}
					data-testid="runtime-persist-note"
				>
					<p class="font-semibold text-text-primary">{runtimePersistStatus.title}</p>
					<p class="mt-1">{runtimePersistStatus.detail}</p>
				</div>

				<RuntimeActivityPanel cards={runtimeActivityCards} />

				<label class="block text-xs text-text-secondary">
					<FieldHelp
						label={localize('Default Session View', '기본 세션 보기')}
						help={runtimeHelp.defaultSessionView}
						testId="runtime-help-default-session-view"
					/>
					<select bind:value={runtimeSessionDefaultView} class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary">
						<option value="full">{localize('full', '전체')}</option>
						<option value="compressed">{localize('compressed', '압축')}</option>
					</select>
				</label>

				<section
					id="runtime-section-provider"
					class="scroll-mt-24 space-y-2 border border-border/60 p-3"
					data-testid="settings-runtime-provider"
				>
					<h3 class="text-xs font-semibold uppercase tracking-[0.08em] text-text-muted">{localize('Provider', '프로바이더')}</h3>
					<label class="block text-xs text-text-secondary">
						<FieldHelp
								label={localize('Summary Provider', '요약 프로바이더')}
							help={runtimeHelp.summaryProvider}
							testId="runtime-help-summary-provider"
						/>
						<select
							bind:value={runtimeProvider}
							onchange={handleRuntimeProviderChange}
							data-testid="runtime-provider-select"
							class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
						>
							<option value="disabled">{localize('disabled', '사용 안 함')}</option>
							<option value="ollama">ollama</option>
							<option value="codex_exec">codex_exec</option>
							<option value="claude_cli">claude_cli</option>
						</select>
					</label>
					<p class="text-[11px] text-text-muted" data-testid="runtime-provider-transport">
						{localize('transport', '전송 방식')}: {currentRuntimeProviderTransport()}
					</p>
					{#if currentRuntimeProviderTransport() === 'http'}
						<label class="block text-xs text-text-secondary">
							<FieldHelp
								label={localize('Endpoint', '엔드포인트')}
								help={runtimeHelp.providerEndpoint}
								testId="runtime-help-provider-endpoint"
							/>
							<input
								bind:value={runtimeEndpoint}
								data-testid="runtime-provider-endpoint"
								class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
							/>
						</label>
						<label class="block text-xs text-text-secondary">
							<FieldHelp
								label={localize('Model', '모델')}
								help={runtimeHelp.providerModel}
								testId="runtime-help-provider-model"
							/>
							<input
								bind:value={runtimeModel}
								data-testid="runtime-provider-model"
								class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
							/>
						</label>
					{:else if currentRuntimeProviderTransport() === 'cli'}
						<label class="block text-xs text-text-secondary">
							<FieldHelp
								label={localize('Model (optional)', '모델 (선택)')}
								help={runtimeHelp.providerModel}
								testId="runtime-help-provider-model"
							/>
							<input
								bind:value={runtimeModel}
								data-testid="runtime-provider-model"
								class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
							/>
						</label>
						<p class="text-[11px] text-text-muted" data-testid="runtime-provider-cli-status">
							{runtimeDetectMessage ?? localize('Run Detect Provider to verify CLI availability.', 'CLI 사용 가능 여부를 확인하려면 프로바이더 감지를 실행하세요.')}
						</p>
					{/if}
				</section>

				<section
					id="runtime-section-prompt"
					class="scroll-mt-24 space-y-2 border border-border/60 p-3"
					data-testid="settings-runtime-prompt"
				>
					<h3 class="text-xs font-semibold uppercase tracking-[0.08em] text-text-muted">{localize('Prompt', '프롬프트')}</h3>
					<label class="block text-xs text-text-secondary">
						<FieldHelp
							label={localize('Prompt Template', '프롬프트 템플릿')}
							help={runtimeHelp.promptTemplate}
							testId="runtime-help-prompt-template"
						/>
						<textarea
							bind:value={runtimePromptTemplate}
							data-testid="runtime-prompt-template"
							rows="6"
							class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
						></textarea>
					</label>
					<div class="flex justify-end">
						<button
							type="button"
							onclick={handleResetPromptTemplate}
							data-testid="runtime-prompt-reset-default"
							class="border border-border px-2 py-1 text-xs text-text-secondary hover:text-text-primary"
						>
							{localize('Reset to default', '기본값으로 재설정')}
						</button>
					</div>
					<label class="block text-xs text-text-secondary">
						<span class="mb-1 block">{localize('Default Template (read-only)', '기본 템플릿 (읽기 전용)')}</span>
						<textarea
							readonly
							value={runtimePromptDefaultTemplate}
							rows="5"
							class="w-full border border-border/70 bg-bg-primary/60 px-2 py-2 text-xs text-text-muted"
						></textarea>
					</label>
				</section>

				<section
					id="runtime-section-response"
					class="scroll-mt-24 space-y-2 border border-border/60 p-3"
					data-testid="settings-runtime-response"
				>
					<h3 class="text-xs font-semibold uppercase tracking-[0.08em] text-text-muted">{localize('Response', '응답')}</h3>
					<div class="grid gap-2 sm:grid-cols-2">
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label={localize('Response Style', '응답 스타일')}
								help={runtimeHelp.responseStyle}
								testId="runtime-help-response-style"
							/>
							<select bind:value={runtimeResponseStyle} class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary">
								<option value="compact">{localize('compact', '간결')}</option>
								<option value="standard">{localize('standard', '표준')}</option>
								<option value="detailed">{localize('detailed', '상세')}</option>
							</select>
						</label>
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label={localize('Output Shape', '출력 형태')}
								help={runtimeHelp.outputShape}
								testId="runtime-help-output-shape"
							/>
							<select bind:value={runtimeOutputShape} class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary">
								<option value="layered">{localize('layered', '레이어별')}</option>
								<option value="file_list">{localize('file_list', '파일 목록')}</option>
								<option value="security_first">{localize('security_first', '보안 우선')}</option>
							</select>
						</label>
					</div>
					<p class="text-[11px] text-text-muted">
						{localize('Desktop source mode is locked to', '데스크톱 원본 모드는')} <code>session_only</code> ({runtimeSourceMode}){localize('.', '로 고정됩니다.')}
					</p>
					<div class="border border-border/70 bg-bg-primary p-2" data-testid="settings-response-preview">
						<p class="mb-2 text-[11px] uppercase tracking-[0.08em] text-text-muted">{localize('Response Preview', '응답 미리보기')}</p>
						<pre class="max-w-full whitespace-pre-wrap text-xs text-text-secondary [overflow-wrap:anywhere]">{responsePreview(runtimeResponseStyle, runtimeOutputShape)}</pre>
					</div>
				</section>

				<section
					id="runtime-section-vector"
					class="scroll-mt-24 space-y-2 border border-border/60 p-3"
					data-testid="settings-runtime-vector"
				>
					<h3 class="text-xs font-semibold uppercase tracking-[0.08em] text-text-muted">{localize('Vector Search', '벡터 검색')}</h3>
					<div class="grid gap-2 sm:grid-cols-2">
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label={localize('Model', '모델')}
								help={runtimeHelp.vectorModel}
								testId="runtime-help-vector-model"
							/>
							<input
								bind:value={runtimeVectorModel}
								data-testid="runtime-vector-model"
								class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
							/>
						</label>
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label={localize('Endpoint', '엔드포인트')}
								help={runtimeHelp.vectorEndpoint}
								testId="runtime-help-vector-endpoint"
							/>
							<input
								bind:value={runtimeVectorEndpoint}
								data-testid="runtime-vector-endpoint"
								class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
							/>
						</label>
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label={localize('Chunking Mode', '청킹 모드')}
								help={runtimeHelp.vectorChunkingMode}
								testId="runtime-help-vector-chunking-mode"
							/>
							<select
								bind:value={runtimeVectorChunkingMode}
								data-testid="runtime-vector-chunking-mode"
								class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
							>
								<option value="auto">{localize('auto', '자동')}</option>
								<option value="manual">{localize('manual', '수동')}</option>
							</select>
						</label>
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label={localize('Chunk Size (lines)', '청크 크기 (줄)')}
								help={runtimeHelp.vectorChunkSize}
								testId="runtime-help-vector-chunk-size"
							/>
							<input
								type="number"
								min="1"
								bind:value={runtimeVectorChunkSizeLines}
								disabled={runtimeVectorChunkingMode === 'auto'}
								data-testid="runtime-vector-chunk-size"
								class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary disabled:opacity-60"
							/>
						</label>
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label={localize('Chunk Overlap (lines)', '청크 겹침 (줄)')}
								help={runtimeHelp.vectorChunkOverlap}
								testId="runtime-help-vector-chunk-overlap"
							/>
							<input
								type="number"
								min="0"
								bind:value={runtimeVectorChunkOverlapLines}
								disabled={runtimeVectorChunkingMode === 'auto'}
								data-testid="runtime-vector-chunk-overlap"
								class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary disabled:opacity-60"
							/>
						</label>
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label={localize('Top K Chunks', '상위 K 청크')}
								help={runtimeHelp.vectorTopKChunks}
								testId="runtime-help-vector-top-k-chunks"
							/>
							<input
								type="number"
								min="1"
								bind:value={runtimeVectorTopKChunks}
								class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
							/>
						</label>
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label={localize('Top K Sessions', '상위 K 세션')}
								help={runtimeHelp.vectorTopKSessions}
								testId="runtime-help-vector-top-k-sessions"
							/>
							<input
								type="number"
								min="1"
								bind:value={runtimeVectorTopKSessions}
								class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
							/>
						</label>
					</div>

					<label class="flex items-center gap-2 text-xs text-text-secondary">
						<input
							type="checkbox"
							bind:checked={runtimeVectorEnabled}
							disabled={!runtimeVectorPreflight?.model_installed}
							data-testid="runtime-vector-enable"
						/>
						<FieldHelp
							inline
							label={localize('Enable semantic vector search', '시맨틱 벡터 검색 사용')}
							help={runtimeHelp.vectorEnable}
							testId="runtime-help-vector-enable"
						/>
					</label>

					<div class="flex flex-wrap items-center gap-2">
						<button
							type="button"
							data-testid="runtime-vector-install"
							onclick={handleVectorInstallModel}
							disabled={runtimeVectorInstalling || runtimeSaving || runtimeLoading}
							class="border border-border px-2 py-1 text-xs text-text-secondary hover:text-text-primary disabled:opacity-60"
						>
							{runtimeVectorInstalling ? localize('Installing...', '설치 중...') : localize('Install model', '모델 설치')}
						</button>
						<button
							type="button"
							data-testid="runtime-vector-reindex"
							onclick={handleVectorReindex}
							disabled={runtimeVectorReindexing || runtimeSaving || runtimeLoading || !runtimeVectorPreflight?.ollama_reachable || !runtimeVectorPreflight?.model_installed}
							class="border border-border px-2 py-1 text-xs text-text-secondary hover:text-text-primary disabled:opacity-60"
						>
							{runtimeVectorReindexing ? localize('Reindexing...', '인덱싱 중...') : localize('Rebuild index', '인덱스 재구성')}
						</button>
					</div>

					<div class="rounded border border-border/60 bg-bg-primary px-3 py-3 text-sm text-text-muted" data-testid="runtime-vector-status">
						<p class="font-medium text-text-primary">
							{localize('Provider', '프로바이더')} {runtimeVectorProvider} · {localize('granularity', '세분화')} {runtimeVectorGranularity} · {localize('chunking', '청킹')} {runtimeVectorChunkingMode}
						</p>
						{#if runtimeVectorPreflight}
							<p class="mt-1">
								{localize('Model', '모델')} {runtimeVectorPreflight.model} · {localize('reachable', '접속 가능')} {boolWord(runtimeVectorPreflight.ollama_reachable)} · {localize('installed', '설치됨')}{' '}
								{boolWord(runtimeVectorPreflight.model_installed)} · {localize('install', '설치')} {runtimeVectorPreflight.install_state}
								({runtimeVectorPreflight.progress_pct}%)
							</p>
							{#if runtimeVectorPreflight.message}
								<p class="mt-1">{runtimeVectorPreflight.message}</p>
							{/if}
						{:else}
							<p class="mt-1">{localize('Vector model status unavailable.', '벡터 모델 상태를 확인할 수 없습니다.')}</p>
						{/if}
						{#if runtimeVectorIndex}
							<p class="mt-1">
								{localize('Index', '인덱스')} {runtimeVectorIndex.state} · {localize('processed', '처리됨')} {runtimeVectorIndex.processed_sessions}/{runtimeVectorIndex.total_sessions}
								{#if progressPercent(runtimeVectorIndex.processed_sessions, runtimeVectorIndex.total_sessions) != null}
									· {progressPercent(runtimeVectorIndex.processed_sessions, runtimeVectorIndex.total_sessions)}%
								{/if}
							</p>
							{#if progressPercent(runtimeVectorIndex.processed_sessions, runtimeVectorIndex.total_sessions) != null}
								<div class="mt-2" data-testid="runtime-vector-progress">
									<div class="flex items-center justify-between text-[11px] text-text-secondary">
										<span>{localize('Embedding progress', '임베딩 진행률')}</span>
										<span>
											{runtimeVectorIndex.processed_sessions}/{runtimeVectorIndex.total_sessions}
											({progressPercent(runtimeVectorIndex.processed_sessions, runtimeVectorIndex.total_sessions)}%)
										</span>
									</div>
									<div class="mt-1 h-2 overflow-hidden rounded bg-border/60">
										<div
											class="h-full bg-accent transition-[width] duration-300"
											style={`width: ${progressPercent(runtimeVectorIndex.processed_sessions, runtimeVectorIndex.total_sessions)}%`}
										></div>
									</div>
								</div>
							{/if}
							{#if runtimeVectorIndex.message}
								<p class="mt-1 whitespace-pre-line">
									{runtimeVectorIndex.state === 'failed'
										? localize(`Last rebuild failure:\n${runtimeVectorIndex.message}`, `마지막 재구성 실패:\n${runtimeVectorIndex.message}`)
										: runtimeVectorIndex.message}
								</p>
								{#if runtimeVectorIndex.state === 'failed' && runtimeVectorPreflight?.ollama_reachable && runtimeVectorPreflight?.model_installed}
									<p class="mt-1 text-[11px] text-text-secondary">
										{localize('Provider looks reachable now. Click', '지금은 프로바이더에 연결할 수 있습니다.')}
										<strong>{localize('Rebuild index', '인덱스 재구성')}</strong>
										{localize('to retry with the current endpoint.', '를 눌러 현재 엔드포인트로 다시 시도하세요.')}
									</p>
								{/if}
							{/if}
							{#if runtimeVectorIndex.started_at || runtimeVectorIndex.finished_at}
								<p class="mt-1 text-[11px] text-text-secondary">
									{localize('started', '시작')}: {formatDate(runtimeVectorIndex.started_at)} | {localize('finished', '종료')}: {formatDate(runtimeVectorIndex.finished_at)}
								</p>
							{/if}
						{/if}
						<div class="mt-2 space-y-1 rounded border border-border/50 bg-bg-secondary/50 px-2 py-2 text-[11px] text-text-secondary">
							<p class="font-semibold uppercase tracking-[0.08em] text-text-muted">{localize('Actions', '조치')}</p>
							{#each vectorStatusGuidance() as line}
								<p>{line}</p>
							{/each}
						</div>
						{#if runtimeVectorError}
							<p
								class="mt-2 whitespace-pre-line rounded border border-error/50 bg-error/10 px-2 py-2 text-sm text-error"
								data-testid="runtime-vector-error"
							>
								{runtimeVectorError}
							</p>
						{/if}
					</div>
				</section>

				<section
					id="runtime-section-change-reader"
					class="scroll-mt-24 space-y-2 border border-border/60 p-3"
					data-testid="settings-runtime-change-reader"
				>
					<h3 class="text-xs font-semibold uppercase tracking-[0.08em] text-text-muted">{localize('Change Reader', '변경 리더')}</h3>
					<div
						class="space-y-1 rounded border border-border/60 bg-bg-primary px-3 py-2 text-[11px] text-text-secondary"
						data-testid="runtime-change-reader-mode-guide"
					>
						<p><span class="font-semibold text-text-primary">{localize('Reader', '리더')}</span>: {localize('text view of the selected change context.', '선택한 변경 맥락의 텍스트 보기입니다.')}</p>
						<p><span class="font-semibold text-text-primary">{localize('Follow-up questions', '후속 질문')}</span>: {localize('ask extra text questions about that same change context.', '같은 변경 맥락에 대해 추가 텍스트 질문을 합니다.')}</p>
						<p><span class="font-semibold text-text-primary">{localize('Voice playback', '음성 재생')}</span>: {localize('read the change reader output aloud with TTS. Requires a Voice API key.', 'TTS로 변경 리더 출력을 소리 내어 읽습니다. Voice API 키가 필요합니다.')}</p>
					</div>
					<label class="flex items-center gap-2 text-xs text-text-secondary">
						<input
							type="checkbox"
							bind:checked={runtimeChangeReaderEnabled}
							data-testid="runtime-change-reader-enable"
						/>
						<FieldHelp
							inline
							label={localize('Enable notebook-style change reading', '노트북형 변경 읽기 사용')}
							help={runtimeHelp.changeReaderEnable}
							testId="runtime-help-change-reader-enable"
						/>
					</label>
					<div class="grid gap-2 sm:grid-cols-2">
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label={localize('Default Scope', '기본 범위')}
								help={runtimeHelp.changeReaderScope}
								testId="runtime-help-change-reader-scope"
							/>
							<select bind:value={runtimeChangeReaderScope} class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary">
								<option value="summary_only">{localize('summary_only', '요약만')}</option>
								<option value="full_context">{localize('full_context', '전체 컨텍스트')}</option>
							</select>
						</label>
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label={localize('Max Context Chars', '최대 컨텍스트 문자 수')}
								help={runtimeHelp.changeReaderMaxContext}
								testId="runtime-help-change-reader-max-context"
							/>
							<input
								type="number"
								min="1"
								bind:value={runtimeChangeReaderMaxContextChars}
								data-testid="runtime-change-reader-max-context"
								class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
							/>
						</label>
					</div>
					<label class="flex items-center gap-2 text-xs text-text-secondary">
						<input
							type="checkbox"
							bind:checked={runtimeChangeReaderQaEnabled}
							disabled={runtimeChangeReaderQaToggleDisabled}
							data-testid="runtime-change-reader-qa"
						/>
						<FieldHelp
							inline
							label={localize('Enable follow-up questions', '후속 질문 사용')}
							help={runtimeHelp.changeReaderQa}
							testId="runtime-help-change-reader-qa"
						/>
					</label>
					<div class="space-y-2 rounded border border-border/60 bg-bg-primary px-2 py-2">
						<p class="text-[11px] font-semibold uppercase tracking-[0.08em] text-text-muted">{localize('Voice Playback', '음성 재생')}</p>
						<p class="text-[11px] text-text-secondary">{runtimeChangeReaderVoiceHint}</p>
						<label class="flex items-center gap-2 text-xs text-text-secondary">
							<input
								type="checkbox"
								bind:checked={runtimeChangeReaderVoiceEnabled}
								disabled={runtimeChangeReaderVoiceToggleDisabled}
								title={runtimeChangeReaderVoiceBlockedReason ?? undefined}
								data-testid="runtime-change-reader-voice-enable"
							/>
							<FieldHelp
								inline
							label={localize('Enable Voice Playback (TTS)', '음성 재생 (TTS) 사용')}
								help={runtimeHelp.changeReaderVoiceEnable}
								testId="runtime-help-change-reader-voice-enable"
							/>
						</label>
						<div class="grid gap-2 sm:grid-cols-3">
							<label class="text-xs text-text-secondary">
								<FieldHelp
									label={localize('Voice Provider', '음성 프로바이더')}
									help={runtimeHelp.changeReaderVoiceProvider}
									testId="runtime-help-change-reader-voice-provider"
								/>
								<select
									bind:value={runtimeChangeReaderVoiceProvider}
									data-testid="runtime-change-reader-voice-provider"
									class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
								>
									<option value="openai">openai</option>
								</select>
							</label>
							<label class="text-xs text-text-secondary">
								<FieldHelp
									label={localize('Voice Model', '음성 모델')}
									help={runtimeHelp.changeReaderVoiceModel}
									testId="runtime-help-change-reader-voice-model"
								/>
								<input
									bind:value={runtimeChangeReaderVoiceModel}
									data-testid="runtime-change-reader-voice-model"
									class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
								/>
							</label>
							<label class="text-xs text-text-secondary">
								<FieldHelp
									label={localize('Voice Name', '음성 이름')}
									help={runtimeHelp.changeReaderVoiceName}
									testId="runtime-help-change-reader-voice-name"
								/>
								<input
									bind:value={runtimeChangeReaderVoiceName}
									data-testid="runtime-change-reader-voice-name"
									class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
								/>
							</label>
						</div>
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label={localize('Voice API Key (write-only)', 'Voice API 키 (쓰기 전용)')}
								help={runtimeHelp.changeReaderVoiceApiKey}
								testId="runtime-help-change-reader-voice-api-key"
							/>
							<input
								type="password"
								placeholder={runtimeChangeReaderVoiceApiKeyConfigured ? localize('Configured (enter new key to rotate)', '설정됨 (교체하려면 새 키 입력)') : localize('Enter API key', 'API 키 입력')}
								bind:value={runtimeChangeReaderVoiceApiKey}
								data-testid="runtime-change-reader-voice-api-key"
								class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
							/>
						</label>
						<p class="text-[11px] text-text-muted" data-testid="runtime-change-reader-voice-key-status">
							{runtimeChangeReaderVoiceKeyStatusLabel}
						</p>
						<p class="text-[11px] text-text-muted" data-testid="runtime-change-reader-voice-requirement">
							{runtimeChangeReaderVoiceHint}
						</p>
					</div>
					<p class="text-[11px] text-text-muted">
						{localize(
							'Uses the configured summary provider when available, then falls back to local heuristic context extraction.',
							'설정된 요약 프로바이더를 우선 사용하고, 사용할 수 없으면 로컬 휴리스틱 컨텍스트 추출로 대체합니다.',
						)}
					</p>
				</section>

				<section
					id="runtime-section-storage"
					class="scroll-mt-24 space-y-2 border border-border/60 p-3"
					data-testid="settings-runtime-storage"
				>
					<h3 class="text-xs font-semibold uppercase tracking-[0.08em] text-text-muted">{localize('Storage', '저장소')}</h3>
					<div class="grid gap-2 sm:grid-cols-2">
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label={localize('Trigger', '트리거')}
								help={runtimeHelp.storageTrigger}
								testId="runtime-help-storage-trigger"
							/>
							<select bind:value={runtimeTriggerMode} class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary">
								<option value="manual">{localize('manual', '수동')}</option>
								<option value="on_session_save">{localize('on_session_save', '세션 저장 시')}</option>
							</select>
						</label>
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label={localize('Backend', '백엔드')}
								help={runtimeHelp.storageBackend}
								testId="runtime-help-storage-backend"
							/>
							<select bind:value={runtimeStorageBackend} class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary">
								<option value="hidden_ref">{localize('hidden_ref (git refs)', 'hidden_ref (git ref)')}</option>
								<option value="local_db">{localize('local_db (sqlite)', 'local_db (sqlite)')}</option>
								<option value="none">{localize('none (no persistence)', 'none (저장 안 함)')}</option>
							</select>
						</label>
					</div>
					<div class="space-y-1 rounded border border-border/60 bg-bg-primary px-2 py-2 text-[11px] text-text-muted" data-testid="runtime-storage-backend-notice">
						<p>
							{localize('Current persisted backend', '현재 저장된 백엔드')}:
							{#if persistedStorageBackend()}
								<code>{persistedStorageBackend()}</code> ({persistedStorageBackendLabel()})
							{:else}
								{localize('unknown', '알 수 없음')}
							{/if}
						</p>
						<p>
							{localize('Selected backend', '선택된 백엔드')}:
							<code>{runtimeStorageBackend}</code> ({storageBackendLabel(runtimeStorageBackend)})
						</p>
						<p>{storageBackendSummary(runtimeStorageBackend)}</p>
						<p>{storageBackendDetails(runtimeStorageBackend)}</p>
						<p data-testid="runtime-storage-transition-note">{storageBackendTransitionDetail()}</p>
						{#if hasPendingStorageBackendChange()}
							<p class="text-text-primary">{localize('Apply this change with', '이 변경을 적용하려면 위의')} <strong>{runtimeSaveLabel()}</strong>{localize('above.', '를 누르세요.')}</p>
						{/if}
					</div>
				</section>

				<section
					id="runtime-section-summary-batch"
					class="scroll-mt-24 space-y-2 border border-border/60 p-3"
					data-testid="settings-runtime-summary-batch"
				>
					<div class="flex flex-wrap items-center justify-between gap-2">
						<h3 class="text-xs font-semibold uppercase tracking-[0.08em] text-text-muted">{localize('Summary Batch', '요약 배치')}</h3>
						<button
							type="button"
							data-testid="runtime-summary-batch-run"
							onclick={handleRunSummaryBatchNow}
							disabled={runtimeSummaryBatchRunning || runtimeSaving || runtimeLoading}
							class="inline-flex h-9 items-center border border-border px-3 text-xs text-text-secondary hover:text-text-primary disabled:opacity-60"
						>
							{runtimeSummaryBatchRunning ? localize('Running...', '실행 중...') : localize('Run now', '지금 실행')}
						</button>
					</div>
					<div class="grid gap-2 sm:grid-cols-3">
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label={localize('Execution Mode', '실행 모드')}
								help={runtimeHelp.batchExecution}
								testId="runtime-help-batch-execution-mode"
							/>
							<select bind:value={runtimeBatchExecutionMode} class="h-9 w-full border border-border bg-bg-primary px-2 text-xs text-text-primary">
								<option value="manual">{localize('manual', '수동')}</option>
								<option value="on_app_start">{localize('on_app_start', '앱 시작 시')}</option>
							</select>
						</label>
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label={localize('Scope', '범위')}
								help={runtimeHelp.batchScope}
								testId="runtime-help-batch-scope"
							/>
							<select bind:value={runtimeBatchScope} class="h-9 w-full border border-border bg-bg-primary px-2 text-xs text-text-primary">
								<option value="recent_days">{localize('recent_days', '최근 일수')}</option>
								<option value="all">{localize('all', '전체')}</option>
							</select>
						</label>
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label={localize('Recent Days', '최근 일수')}
								help={runtimeHelp.batchRecentDays}
								testId="runtime-help-batch-recent-days"
							/>
							<input
								type="number"
								min="1"
								bind:value={runtimeBatchRecentDays}
								disabled={runtimeBatchScope === 'all'}
								data-testid="runtime-summary-batch-recent-days"
								class="h-9 w-full border border-border bg-bg-primary px-2 text-xs text-text-primary disabled:opacity-60"
							/>
						</label>
					</div>
					<div class="rounded border border-border/60 bg-bg-primary px-2 py-2 text-[11px] text-text-muted" data-testid="runtime-summary-batch-status">
						{#if runtimeSummaryBatchStatus}
							<p>
								{localize('state', '상태')}: {runtimeSummaryBatchStatus.state} | {summaryBatchProgressLabel(runtimeSummaryBatchStatus)}
							</p>
							{#if progressPercent(runtimeSummaryBatchStatus.processed_sessions, runtimeSummaryBatchStatus.total_sessions) != null}
								<div class="mt-2" data-testid="runtime-summary-batch-progress">
									<div class="flex items-center justify-between text-[11px] text-text-secondary">
										<span>{localize('Batch progress', '배치 진행률')}</span>
										<span>
											{runtimeSummaryBatchStatus.processed_sessions}/{runtimeSummaryBatchStatus.total_sessions}
											({progressPercent(runtimeSummaryBatchStatus.processed_sessions, runtimeSummaryBatchStatus.total_sessions)}%)
										</span>
									</div>
									<div class="mt-1 h-2 overflow-hidden rounded bg-border/60">
										<div
											class="h-full bg-accent transition-[width] duration-300"
											style={`width: ${progressPercent(runtimeSummaryBatchStatus.processed_sessions, runtimeSummaryBatchStatus.total_sessions)}%`}
										></div>
									</div>
								</div>
							{/if}
							{#if runtimeSummaryBatchStatus.message}
								<p class="mt-1">{runtimeSummaryBatchStatus.message}</p>
							{/if}
							<p class="mt-1">
								{localize('started', '시작')}: {formatDate(runtimeSummaryBatchStatus.started_at)} | {localize('finished', '종료')}: {formatDate(runtimeSummaryBatchStatus.finished_at)}
							</p>
						{:else}
							<p>{localize('batch status unavailable.', '배치 상태를 확인할 수 없습니다.')}</p>
						{/if}
					</div>
				</section>

				<section
					id="runtime-section-lifecycle"
					class="scroll-mt-24 space-y-2 border border-border/60 p-3"
					data-testid="settings-runtime-lifecycle"
				>
					<h3 class="text-xs font-semibold uppercase tracking-[0.08em] text-text-muted">{localize('Data Lifecycle', '데이터 수명주기')}</h3>
					<label class="flex items-center gap-2 text-xs text-text-secondary">
						<input type="checkbox" bind:checked={runtimeLifecycleEnabled} data-testid="runtime-lifecycle-enable" />
						<FieldHelp
							inline
							label={localize('Enable periodic lifecycle cleanup', '주기적 수명주기 정리 사용')}
							help={runtimeHelp.lifecycleEnable}
							testId="runtime-help-lifecycle-enable"
						/>
					</label>
					<div class="grid gap-2 sm:grid-cols-3">
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label={localize('Session TTL (days)', '세션 TTL (일)')}
								help={runtimeHelp.sessionTtl}
								testId="runtime-help-session-ttl"
							/>
							<input
								type="number"
								min="1"
								bind:value={runtimeSessionTtlDays}
								data-testid="runtime-lifecycle-session-ttl"
								class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
							/>
						</label>
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label={localize('Summary TTL (days)', '요약 TTL (일)')}
								help={runtimeHelp.summaryTtl}
								testId="runtime-help-summary-ttl"
							/>
							<input
								type="number"
								min="1"
								bind:value={runtimeSummaryTtlDays}
								data-testid="runtime-lifecycle-summary-ttl"
								class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
							/>
						</label>
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label={localize('Cleanup Interval (sec)', '정리 주기 (초)')}
								help={runtimeHelp.cleanupInterval}
								testId="runtime-help-cleanup-interval"
							/>
							<input
								type="number"
								min="60"
								bind:value={runtimeCleanupIntervalSecs}
								data-testid="runtime-lifecycle-interval"
								class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
							/>
						</label>
					</div>
					<div
						class="rounded border border-border/60 bg-bg-primary px-2 py-2 text-[11px] text-text-muted"
						data-testid="runtime-lifecycle-status"
					>
						{#if runtimeLifecycleStatus}
							<p>
								{localize('state', '상태')}: {runtimeLifecycleStatus.state} | {localize('deleted', '삭제됨')}: {runtimeLifecycleStatus.deleted_sessions} {localize('sessions', '세션')} /
								{runtimeLifecycleStatus.deleted_summaries} {localize('summaries', '요약')}
							</p>
							<p class="mt-1">
								{localize('next run', '다음 실행')}: {lifecycleNextRunLabel()} | {localize('interval', '주기')}: {formatIntervalSeconds(runtimeCleanupIntervalSecs)}
							</p>
							{#if runtimeLifecycleStatus.message}
								<p class="mt-1">{runtimeLifecycleStatus.message}</p>
							{/if}
							<p class="mt-1">
								{localize('started', '시작')}: {formatDate(runtimeLifecycleStatus.started_at)} | {localize('finished', '종료')}: {formatDate(runtimeLifecycleStatus.finished_at)}
							</p>
						{:else}
							<p>{localize('No lifecycle cleanup runs recorded yet.', '아직 수명주기 정리 실행 기록이 없습니다.')}</p>
						{/if}
					</div>
					<div class="overflow-x-auto border border-border/70 bg-bg-primary p-2">
						<p class="mb-2 text-[11px] uppercase tracking-[0.08em] text-text-muted">{localize('Root / Dependent Cleanup', '루트 / 종속 항목 정리')}</p>
						<table class="w-full text-left text-[11px] text-text-secondary">
							<thead>
								<tr class="text-text-muted">
									<th class="pb-1">{localize('Root', '루트')}</th>
									<th class="pb-1">{localize('Dependents', '종속 항목')}</th>
									<th class="pb-1">{localize('Rule', '규칙')}</th>
								</tr>
							</thead>
							<tbody>
								<tr>
									<td class="pr-4 align-top">{localize('session', '세션')}</td>
									<td class="pr-4 align-top">{localize('summary, vector chunks, vector index, body cache, session links', '요약, 벡터 청크, 벡터 인덱스, 본문 캐시, 세션 링크')}</td>
									<td class="align-top">{localize('Deleting a session also deletes all dependents', '세션을 삭제하면 모든 종속 항목도 삭제됩니다')}</td>
								</tr>
								<tr>
									<td class="pr-4 align-top">{localize('summary', '요약')}</td>
									<td class="pr-4 align-top">{localize('summary metadata (hidden_ref/local row)', '요약 메타데이터 (hidden_ref/로컬 행)')}</td>
									<td class="align-top">{localize('Deleting a summary keeps the session', '요약을 삭제해도 세션은 유지됩니다')}</td>
								</tr>
							</tbody>
						</table>
					</div>
				</section>

				</div>

				<RuntimeQuickMenu
					draftDirty={runtimeDraftDirty}
					runtimeSaving={runtimeSaving}
					runtimeLoading={runtimeLoading}
					saveLabel={runtimeSaveLabel()}
					provider={runtimeProvider}
					storageBackend={runtimeStorageBackend}
					sessionDefaultView={runtimeSessionDefaultView}
					providerTransport={currentRuntimeProviderTransport()}
					batchScopeLabel={runtimeQuickBatchScopeLabel}
					summaryTriggerAuto={runtimeTriggerMode === 'on_session_save'}
					summaryTriggerDetail={runtimeQuickSummaryTriggerDetail}
					batchAuto={runtimeBatchExecutionMode === 'on_app_start'}
					batchDetail={runtimeQuickBatchDetail}
					batchStatusDetail={runtimeQuickBatchStatusDetail}
					lifecycleEnabled={runtimeLifecycleEnabled}
					lifecycleDetail={runtimeQuickLifecycleDetail}
					lifecycleResultDetail={lifecycleResultLabel(runtimeLifecycleStatus)}
					lifecycleNextDetail={runtimeQuickLifecycleNextDetail}
					vectorEnabled={runtimeVectorEnabled}
					vectorToggleDisabled={!runtimeVectorPreflight?.model_installed}
					vectorDetail={runtimeQuickVectorDetail}
					vectorStatusDetail={runtimeQuickVectorStatusDetail}
					changeReaderEnabled={runtimeChangeReaderEnabled}
					changeReaderDetail={runtimeQuickChangeReaderDetail}
					changeReaderQaEnabled={runtimeChangeReaderQaEnabled}
					changeReaderQaDisabled={runtimeChangeReaderQaToggleDisabled}
					changeReaderVoiceEnabled={runtimeChangeReaderVoiceEnabled}
					changeReaderVoiceDisabled={runtimeChangeReaderVoiceToggleDisabled}
					changeReaderVoiceBlockedReason={runtimeChangeReaderVoiceBlockedReason}
					changeReaderVoiceSummary={runtimeChangeReaderVoiceSummary}
					jumpLinks={runtimeQuickJumpLinks}
					onReset={handleResetRuntimeDraft}
					onSave={handleSaveRuntimeSettings}
					onProviderChange={updateRuntimeProvider}
					onStorageBackendChange={updateRuntimeStorageBackend}
					onToggleSummaryTrigger={toggleRuntimeSummaryTrigger}
					onToggleBatch={toggleRuntimeBatchExecution}
					onToggleLifecycle={toggleRuntimeLifecycle}
					onToggleVector={toggleRuntimeVector}
					onToggleChangeReader={toggleRuntimeChangeReader}
					onToggleChangeReaderQa={toggleRuntimeChangeReaderQa}
					onToggleChangeReaderVoice={toggleRuntimeChangeReaderVoice}
					onJumpToSection={setActiveSettingsSection}
				/>
			</div>
		{/if}
		{#if runtimeError}
			<p class="mt-2 text-xs text-error">{runtimeError}</p>
		{/if}
		{#if runtimeDetectMessage}
			<p class="mt-2 text-xs text-text-secondary">{runtimeDetectMessage}</p>
		{/if}
		{#if runtimeDraftDirty || runtimeSaving}
			<div
				class="sticky bottom-4 z-20 mt-4 flex flex-wrap items-center justify-between gap-3 border border-border bg-bg-secondary px-3 py-3 shadow-[0_10px_30px_rgba(15,23,42,0.18)]"
				data-testid="runtime-draft-bar"
			>
				<div class="min-w-0 flex-1">
					<p class="text-xs font-semibold text-text-primary">{runtimePersistStatus.title}</p>
					<p class="mt-1 text-xs text-text-secondary">{runtimePersistStatus.detail}</p>
				</div>
				<div class="flex items-center gap-2">
					<button
						type="button"
						data-testid="runtime-reset-draft"
						onclick={handleResetRuntimeDraft}
						disabled={runtimeSaving}
						class="inline-flex h-9 items-center border border-border px-3 text-xs font-semibold text-text-secondary hover:text-text-primary disabled:opacity-60"
					>
						{localize('Reset Draft', '초안 초기화')}
					</button>
					<button
						type="button"
						data-testid="runtime-save-sticky"
						onclick={handleSaveRuntimeSettings}
						disabled={runtimeSaving || runtimeLoading}
						class="inline-flex h-9 items-center border border-transparent bg-accent px-3 text-xs font-semibold text-white hover:bg-accent/85 disabled:opacity-60"
					>
						{runtimeSaveLabel()}
					</button>
				</div>
			</div>
		{/if}
	</section>
		</div>
	</div>
</div>

<FloatingJobStatus jobs={floatingJobs} />
