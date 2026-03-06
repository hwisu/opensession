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

const {
	onNavigate = (path: string) => {
		window.location.assign(path);
	},
}: {
	onNavigate?: (path: string) => void;
} = $props();

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
	return `${status.processed_sessions}/${status.total_sessions} sessions (${pct}%)`;
}

function summaryBatchProgressLabel(
	status: DesktopSummaryBatchStatusResponse | null,
): string | null {
	if (!status) return null;
	if (status.total_sessions <= 0) {
		return status.failed_sessions > 0 ? `failed ${status.failed_sessions}` : 'no queued sessions';
	}
	return `${status.processed_sessions}/${status.total_sessions} sessions · failed ${status.failed_sessions}`;
}

function formatIntervalSeconds(seconds: number): string {
	if (seconds < 60) return `${seconds}s`;
	if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
	if (seconds % 3600 === 0) return `${Math.floor(seconds / 3600)}h`;
	return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
}

function lifecycleResultLabel(status: DesktopLifecycleCleanupStatusResponse | null): string {
	if (!status) return 'No lifecycle cleanup runs recorded yet.';
	return `${status.deleted_sessions} sessions deleted · ${status.deleted_summaries} summaries removed`;
}

function lifecycleNextRunLabel(): string {
	if (!runtimeLifecycleEnabled) return 'paused';
	if (isLifecycleCleanupRunning(runtimeLifecycleStatus)) return 'running now';
	const anchor = runtimeLifecycleStatus?.finished_at ?? runtimeLifecycleStatus?.started_at;
	if (!anchor) {
		return `after app start, then every ${formatIntervalSeconds(runtimeCleanupIntervalSecs)}`;
	}
	const next = new Date(new Date(anchor).getTime() + runtimeCleanupIntervalSecs * 1000);
	if (Number.isNaN(next.getTime())) {
		return `every ${formatIntervalSeconds(runtimeCleanupIntervalSecs)}`;
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
	if (!runtimeChangeReaderEnabled) return 'Enable Change Reader first.';
	if (!runtimeChangeReaderVoiceApiKeyReady) return 'Add a Voice API key first.';
	return null;
});

const runtimeChangeReaderVoiceKeyStatusLabel = $derived.by(() => {
	if (runtimeChangeReaderVoiceApiKey.trim().length > 0) {
		return 'Voice API key: pending save';
	}
	return runtimeChangeReaderVoiceApiKeyConfigured
		? 'Voice API key: configured'
		: 'Voice API key: missing';
});

const runtimeChangeReaderVoiceHint = $derived.by(() => {
	if (runtimeChangeReaderVoiceBlockedReason) {
		return `${runtimeChangeReaderVoiceBlockedReason} Voice playback only reads the change reader output aloud.`;
	}
	return 'Voice playback reads the same change reader output aloud. It does not change summaries or follow-up Q&A.';
});

const runtimeChangeReaderVoiceSummary = $derived.by(() => {
	const base = `${runtimeChangeReaderVoiceProvider} · ${runtimeChangeReaderVoiceModel}`;
	if (!runtimeChangeReaderVoiceApiKeyReady) {
		return `${base} · API key required`;
	}
	return base;
});

const floatingJobs = $derived.by(() => {
	const jobs: Array<{ id: string; label: string; detail: string }> = [];
	if (runtimeSaving) {
		jobs.push({
			id: 'runtime-save',
			label: 'Saving runtime settings',
			detail: 'Storage migration and runtime validation can take a while. Continue using the page.',
		});
	}
	if (runtimeVectorInstalling) {
		jobs.push({
			id: 'vector-install',
			label: 'Installing vector model',
			detail: 'Model pull is running in background.',
		});
	}
	if (runtimeVectorReindexing) {
		const progress = vectorIndexProgressLabel(runtimeVectorIndex);
		jobs.push({
			id: 'vector-reindex',
			label: 'Rebuilding vector index',
			detail: progress
				? `Session embeddings are being rebuilt in background. Processed ${progress}.`
				: 'Session embeddings are being rebuilt in background.',
		});
	}
	if (runtimeSummaryBatchRunning) {
		const progress = summaryBatchProgressLabel(runtimeSummaryBatchStatus);
		jobs.push({
			id: 'summary-batch',
			label: 'Running summary batch',
			detail: progress
				? `Generating summaries in background. ${progress}.`
				: 'Generating summaries in background.',
		});
	}
	if (isLifecycleCleanupRunning(runtimeLifecycleStatus)) {
		jobs.push({
			id: 'lifecycle-cleanup',
			label: 'Running lifecycle cleanup',
			detail: runtimeLifecycleStatus?.message ?? 'Removing expired sessions and summaries.',
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
	if (backend === 'hidden_ref') return 'git hidden refs';
	if (backend === 'local_db') return 'local SQLite';
	return 'ephemeral only';
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
		return 'Read and write persisted summaries from git hidden refs in each session repository.';
	}
	if (backend === 'local_db') {
		return 'Read and write persisted summaries from the local SQLite table `session_semantic_summaries`.';
	}
	return 'Do not read or write persisted summaries. Results are generated only for the current request.';
}

function storageBackendDetails(backend: DesktopSummaryStorageBackend): string {
	if (backend === 'hidden_ref') {
		return 'Best when the session belongs to a git repository and you want git-backed summary history alongside the repo.';
	}
	if (backend === 'local_db') {
		return 'Best when you want machine-local persistence without writing anything into git refs.';
	}
	return 'Use this only when you want no persistence. Existing stored summaries are left where they already are.';
}

function storageBackendTransitionDetail(): string {
	const current = persistedStorageBackend();
	if (!current) {
		return 'Load runtime settings to inspect storage migration behavior.';
	}
	if (current === runtimeStorageBackend) {
		return 'No storage backend switch is pending. Click Save Runtime only if you want to persist other runtime edits.';
	}
	if (current === 'none') {
		return `On next save, new summaries will persist to ${storageBackendLabel(runtimeStorageBackend)}. Nothing is copied because the current backend stores no persisted summaries.`;
	}
	if (runtimeStorageBackend === 'none') {
		return `On next save, desktop stops reading and writing persisted summaries. Existing summaries stay in ${storageBackendLabel(current)}. Nothing is migrated or deleted automatically.`;
	}
	return `On next save, existing summaries are copied from ${storageBackendLabel(current)} to ${storageBackendLabel(runtimeStorageBackend)}. Existing source copies are kept.`;
}

function runtimeSaveLabel(): string {
	if (runtimeSaving) return 'Saving...';
	const current = persistedStorageBackend();
	if (!current || current === runtimeStorageBackend) {
		return 'Save Runtime';
	}
	if (
		(current === 'hidden_ref' && runtimeStorageBackend === 'local_db') ||
		(current === 'local_db' && runtimeStorageBackend === 'hidden_ref')
	) {
		return 'Save Runtime + Migrate';
	}
	return 'Save Runtime + Apply Storage';
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
	{ id: 'runtime-section-activity', label: 'Activity' },
	{ id: 'runtime-section-provider', label: 'Provider' },
	{ id: 'runtime-section-vector', label: 'Vector' },
	{ id: 'runtime-section-change-reader', label: 'Reader' },
	{ id: 'runtime-section-storage', label: 'Storage' },
	{ id: 'runtime-section-summary-batch', label: 'Batch' },
	{ id: 'runtime-section-lifecycle', label: 'TTL' },
] as const;

const settingsNavItems = $derived.by((): SettingsSectionNavItem[] => {
	const items = [
		{
			id: 'settings-section-overview',
			label: 'Overview',
			detail: 'Page summary and account context',
			visible: true,
		},
		{
			id: 'settings-section-profile',
			label: 'Profile',
			detail: 'Identity and linked providers',
			visible: authApiEnabled && !authRequired,
		},
		{
			id: 'settings-section-api-key',
			label: 'API Key',
			detail: 'CLI and automation access',
			visible: authApiEnabled && !authRequired,
		},
		{
			id: 'settings-section-git-credentials',
			label: 'Git Auth',
			detail: 'Private repository credentials',
			visible: authApiEnabled && !authRequired,
		},
		{
			id: 'settings-section-runtime',
			label: 'Runtime',
			detail: 'Desktop summary controls',
			visible: true,
		},
		{
			id: 'runtime-section-activity',
			label: 'Activity',
			detail: 'Live job and cleanup status',
			visible: runtimeSupported,
		},
		{
			id: 'runtime-section-provider',
			label: 'Provider',
			detail: 'Summary backend and transport',
			visible: runtimeSupported,
		},
		{
			id: 'runtime-section-prompt',
			label: 'Prompt',
			detail: 'Template and reset controls',
			visible: runtimeSupported,
		},
		{
			id: 'runtime-section-response',
			label: 'Response',
			detail: 'Style, shape, preview',
			visible: runtimeSupported,
		},
		{
			id: 'runtime-section-vector',
			label: 'Vector',
			detail: 'Embeddings and index jobs',
			visible: runtimeSupported,
		},
		{
			id: 'runtime-section-change-reader',
			label: 'Reader',
			detail: 'Text, Q&A, and voice',
			visible: runtimeSupported,
		},
		{
			id: 'runtime-section-storage',
			label: 'Storage',
			detail: 'Persistence backend and trigger',
			visible: runtimeSupported,
		},
		{
			id: 'runtime-section-summary-batch',
			label: 'Batch',
			detail: 'Background summary generation',
			visible: runtimeSupported,
		},
		{
			id: 'runtime-section-lifecycle',
			label: 'Lifecycle',
			detail: 'TTL and cleanup intervals',
			visible: runtimeSupported,
		},
	];
	return items.filter((item) => item.visible);
});

const runtimeQuickBatchScopeLabel = $derived.by(() =>
	runtimeBatchScope === 'all' ? 'all sessions' : `${runtimeBatchRecentDays} days`,
);

const runtimeActivityCards = $derived.by((): RuntimeActivityCard[] => {
	const filterLines = (lines: Array<string | null | undefined>): string[] =>
		lines.filter((line): line is string => typeof line === 'string' && line.length > 0);

	return [
		{
			testId: 'runtime-activity-vector',
			title: 'Vector index',
			subtitle: `${runtimeVectorProvider} · ${runtimeVectorModel}`,
			badges: [
				{
					label: runtimeVectorEnabled ? 'On' : 'Off',
					tone: runtimeVectorEnabled ? 'enabled' : 'disabled',
				},
				{
					label: runtimeVectorIndex?.state ?? 'idle',
					tone: activityStateTone(runtimeVectorIndex?.state),
				},
			],
			lines: filterLines([
				`Provider reachable ${runtimeVectorPreflight?.ollama_reachable ? 'yes' : 'no'} · model installed ${runtimeVectorPreflight?.model_installed ? 'yes' : 'no'}`,
				vectorIndexProgressLabel(runtimeVectorIndex) ?? 'No rebuild progress recorded yet.',
				runtimeVectorIndex?.message,
			]),
			timestampLine: `started ${formatDate(runtimeVectorIndex?.started_at)} · finished ${formatDate(runtimeVectorIndex?.finished_at)}`,
		},
		{
			testId: 'runtime-activity-summary-batch',
			title: 'Summary batch',
			subtitle: runtimeBatchExecutionMode === 'on_app_start' ? 'auto on app start' : 'manual only',
			badges: [
				{
					label: runtimeBatchExecutionMode === 'on_app_start' ? 'Auto' : 'Manual',
					tone: runtimeBatchExecutionMode === 'on_app_start' ? 'enabled' : 'disabled',
				},
				{
					label: runtimeSummaryBatchStatus?.state ?? 'idle',
					tone: activityStateTone(runtimeSummaryBatchStatus?.state),
				},
			],
			lines: filterLines([
				`scope ${runtimeBatchScope === 'all' ? 'all sessions' : `${runtimeBatchRecentDays} days`}`,
				summaryBatchProgressLabel(runtimeSummaryBatchStatus) ?? 'No batch runs recorded yet.',
				runtimeSummaryBatchStatus?.message,
			]),
			timestampLine: `started ${formatDate(runtimeSummaryBatchStatus?.started_at)} · finished ${formatDate(runtimeSummaryBatchStatus?.finished_at)}`,
		},
		{
			testId: 'runtime-activity-lifecycle',
			title: 'Lifecycle cleanup',
			subtitle: `${runtimeSessionTtlDays}d session TTL · ${runtimeSummaryTtlDays}d summary TTL`,
			badges: [
				{
					label: runtimeLifecycleEnabled ? 'On' : 'Off',
					tone: runtimeLifecycleEnabled ? 'enabled' : 'disabled',
				},
				{
					label: runtimeLifecycleStatus?.state ?? 'idle',
					tone: activityStateTone(runtimeLifecycleStatus?.state),
				},
			],
			lines: filterLines([
				`interval ${formatIntervalSeconds(runtimeCleanupIntervalSecs)} · next ${lifecycleNextRunLabel()}`,
				lifecycleResultLabel(runtimeLifecycleStatus),
				runtimeLifecycleStatus?.message,
			]),
			timestampLine: `started ${formatDate(runtimeLifecycleStatus?.started_at)} · finished ${formatDate(runtimeLifecycleStatus?.finished_at)}`,
		},
	];
});

const runtimeQuickSummaryTriggerDetail = $derived.by(() =>
	runtimeTriggerMode === 'on_session_save' ? 'runs automatically on new saves' : 'manual only',
);

const runtimeQuickBatchDetail = $derived.by(
	() => `scope ${runtimeBatchScope === 'all' ? 'all sessions' : `${runtimeBatchRecentDays} days`}`,
);

const runtimeQuickBatchStatusDetail = $derived.by(
	() => summaryBatchProgressLabel(runtimeSummaryBatchStatus) ?? 'No batch runs yet.',
);

const runtimeQuickLifecycleDetail = $derived.by(
	() => `${runtimeSessionTtlDays}d session TTL · every ${formatIntervalSeconds(runtimeCleanupIntervalSecs)}`,
);

const runtimeQuickLifecycleNextDetail = $derived.by(() => `next ${lifecycleNextRunLabel()}`);

const runtimeQuickVectorDetail = $derived.by(() => {
	const base = `${runtimeVectorProvider} · ${runtimeVectorModel}`;
	if (runtimeVectorPreflight && !runtimeVectorPreflight.model_installed) {
		return `${base} · model missing`;
	}
	return base;
});

const runtimeQuickVectorStatusDetail = $derived.by(
	() => vectorIndexProgressLabel(runtimeVectorIndex) ?? `index ${runtimeVectorIndex?.state ?? 'idle'}`,
);

const runtimeQuickChangeReaderDetail = $derived.by(
	() =>
		`text reader · ${runtimeChangeReaderScope} · ${runtimeChangeReaderMaxContextChars.toLocaleString()} chars`,
);

const runtimeHelp = {
	defaultSessionView:
		'full shows the complete raw session. compressed prioritizes semantic summary + condensed context.',
	summaryProvider:
		'disabled turns off summary generation. ollama uses local HTTP inference. codex_exec/claude_cli run local CLI providers.',
	providerEndpoint: 'HTTP base URL for ollama or other local model server.',
	providerModel: 'Model name used by the selected provider.',
	promptTemplate:
		'Template passed to the summary generator. Keep placeholders used by your runtime prompt contract.',
	responseStyle:
		'compact = shortest output, standard = balanced, detailed = richer narrative and context.',
	outputShape:
		'layered groups by layer, file_list focuses per-file changes, security_first prioritizes auth/security impact.',
	vectorModel: 'Embedding model name used for vector indexing.',
	vectorEndpoint: 'Endpoint for local embedding provider (typically Ollama).',
	vectorChunkingMode:
		'auto selects chunk size/overlap from session length best-practice rules. manual uses the fixed values below.',
	vectorChunkSize: 'Number of lines per semantic chunk before embedding.',
	vectorChunkOverlap: 'Overlapping lines preserved between adjacent chunks.',
	vectorTopKChunks: 'Maximum chunk candidates retrieved per query.',
	vectorTopKSessions: 'Maximum sessions surfaced after chunk ranking.',
	vectorEnable:
		'Turns on semantic retrieval in search and change analysis. Requires model install and index build.',
	changeReaderEnable:
		'Turns on the text-based change reader so you can inspect what changed and why across a session.',
	changeReaderScope:
		'summary_only reads compressed context. full_context expands to broader session context when needed.',
	changeReaderMaxContext: 'Upper bound of context text loaded for change reading.',
	changeReaderQa:
		'Adds follow-up text Q&A on top of the selected change reader context when the reader is enabled.',
	changeReaderVoiceEnable:
		'Reads the current change reader output aloud with TTS. Requires a Voice API key and does not change summaries or Q&A.',
	changeReaderVoiceProvider: 'Voice provider for TTS playback.',
	changeReaderVoiceModel: 'TTS model used when generating speech audio.',
	changeReaderVoiceName: 'Voice preset name used by the provider.',
	changeReaderVoiceApiKey:
		'Write-only API key for voice playback. Required before voice playback can be enabled. Leave empty to keep the current stored key.',
	storageTrigger: 'manual runs only when explicitly requested. on_session_save runs automatically on new saves.',
	storageBackend:
		'Where summaries are read from and written to after you click Save Runtime. Switching between hidden_ref and local_db copies existing summaries into the selected backend on save. Switching to none does not migrate or delete existing stored summaries.',
	batchExecution:
		'manual means run only when clicking Run now. on_app_start runs once automatically at desktop startup.',
	batchScope:
		'recent_days targets only recent sessions. all targets every known session regardless of recency.',
	batchRecentDays: 'Applies only when scope is recent_days. Minimum is 1 day.',
	lifecycleEnable:
		'Enables periodic TTL cleanup for session roots and summary artifacts using the rules below.',
	sessionTtl: 'Sessions older than this threshold become cleanup candidates.',
	summaryTtl: 'Summary artifacts older than this threshold become cleanup candidates.',
	cleanupInterval: 'Seconds between periodic lifecycle cleanup runs.',
};

function responsePreview(
	style: DesktopSummaryResponseStyle,
	shape: DesktopSummaryOutputShape,
): string {
	const changesPrefix =
		style === 'compact'
			? 'Updated session summary pipeline.'
			: style === 'detailed'
				? 'Refactored the desktop summary pipeline, split provider/prompt/response/storage concerns, and updated hidden-ref persistence semantics.'
				: 'Updated desktop summary pipeline with clearer runtime settings.';
	const security =
		shape === 'security_first'
			? 'Credential paths were isolated and storage policy now defaults to hidden_ref.'
			: 'none detected';

	const files =
		shape === 'file_list'
			? ['desktop/src-tauri/src/main.rs', 'packages/ui/src/components/SettingsPage.svelte']
			: ['desktop/src-tauri/src/main.rs'];
	const layer = shape === 'file_list' ? 'application' : 'presentation';

	return JSON.stringify(
		{
			changes: changesPrefix,
			auth_security: security,
			layer_file_changes: [
				{
					layer,
					summary:
						style === 'compact'
							? 'Settings/runtime summary flow updated.'
							: 'Runtime settings and summary persistence behavior were updated.',
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
	return parsed.toLocaleString();
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
		if (reason) lines.push(`Reason: ${reason}`);
		if (status != null) lines.push(`HTTP: ${status}`);
		if (batchReason) lines.push(`Batch reason: ${batchReason}`);
		if (batchStatus != null) lines.push(`Batch HTTP: ${batchStatus}`);
		if (hint) lines.push(`Action: ${hint}`);
		if (model) lines.push(`Model: ${model}`);
		if (endpoint) lines.push(`Endpoint: ${endpoint}`);
		if (batchEndpoint) lines.push(`Batch endpoint: ${batchEndpoint}`);
		return lines.join('\n');
	}
	return normalizeError(err, fallback);
}

function vectorStatusGuidance(): string[] {
	if (!runtimeVectorPreflight) {
		return ['Run vector preflight to inspect provider and model readiness.'];
	}
	const guidance: string[] = [];
	if (!runtimeVectorPreflight.ollama_reachable) {
		guidance.push('Install Ollama: https://ollama.com/download');
		guidance.push('Start provider: run `ollama serve` and retry preflight.');
	}
	if (!runtimeVectorPreflight.model_installed) {
		guidance.push(
			`Install model: run \`ollama pull ${runtimeVectorPreflight.model}\` or click "Install model".`,
		);
	}
	if (runtimeVectorIndex?.state === 'failed') {
		guidance.push('Rebuild index: click "Rebuild index" after fixing provider/model issues.');
	}
	if (guidance.length === 0) {
		guidance.push('Vector pipeline is ready.');
	}
	return guidance;
}

async function loadSettings() {
	loading = true;
	error = null;
	authRequired = false;
	try {
		const capabilities = await getApiCapabilities();
		authApiEnabled = capabilities.auth_enabled;
	} catch {
		authApiEnabled = false;
	}

	if (!authApiEnabled) {
		settings = null;
		credentials = [];
		loading = false;
		return;
	}

	if (!isAuthenticated()) {
		authRequired = true;
		loading = false;
		return;
	}

	try {
		settings = await getSettings();
		await loadGitCredentials();
	} catch (err) {
		settings = null;
		if (err instanceof ApiError && (err.status === 401 || err.status === 403)) {
			authRequired = true;
		} else {
			error = normalizeError(err, 'Failed to load settings');
		}
	} finally {
		loading = false;
	}
}

async function loadGitCredentials() {
	credentialsLoading = true;
	credentialsError = null;
	credentialsSupported = true;
	try {
		credentials = await listGitCredentials();
	} catch (err) {
		credentials = [];
		if (err instanceof ApiError && err.status === 404) {
			credentialsSupported = false;
			return;
		}
		credentialsError = normalizeError(err, 'Failed to load git credentials');
	} finally {
		credentialsLoading = false;
	}
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
	runtimeError = null;
	runtimeVectorError = null;
	runtimeSupported = true;
	try {
		const settings = await getRuntimeSettings();
		runtimeSettings = settings;
		applyRuntimeSettingsToDraft(settings);
		await refreshLifecycleCleanupStatus();
		await refreshSummaryBatchStatus();
		await refreshVectorPreflight();
		await refreshVectorIndexStatus();
	} catch (err) {
		runtimeSettings = null;
		if (err instanceof ApiError && err.status === 501) {
			runtimeSupported = false;
		} else {
			runtimeError = normalizeError(err, 'Failed to load runtime settings');
		}
	} finally {
		runtimeLoading = false;
	}
}

async function refreshLifecycleCleanupStatus(surfaceError: boolean = true): Promise<boolean> {
	try {
		runtimeLifecycleStatus = await getLifecycleCleanupStatus();
		return true;
	} catch (err) {
		runtimeLifecycleStatus = null;
		if (surfaceError) {
			runtimeError = normalizeError(err, 'Failed to fetch lifecycle cleanup status');
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
			title: 'Loading runtime config',
			detail: 'Fetching the current persisted desktop runtime settings.',
		};
	}
	if (runtimeDraftDirty) {
		return {
			title: 'Unsaved runtime changes',
			detail:
				'Checkbox, select, and input edits are drafts until you click Save Runtime. Reopening Settings reloads the last persisted values.',
		};
	}
	return {
		title: 'Runtime config is persisted',
		detail:
			'Current values match the saved desktop runtime config. New edits stay local to this page until you save them.',
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
	runtimeDetectMessage = 'Discarded unsaved runtime edits.';
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
		runtimeDetectMessage = 'Runtime settings saved and will persist when you reopen Settings.';
	} catch (err) {
		runtimeError = normalizeError(err, 'Failed to save runtime settings');
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
			runtimeDetectMessage = 'No local provider detected.';
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
		runtimeDetectMessage = `Detected and applied provider: ${detected.provider}`;
	} catch (err) {
		runtimeError = normalizeError(err, 'Failed to detect/apply local provider');
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
		runtimeError = normalizeError(err, 'Failed to fetch summary batch status');
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
		runtimeError = normalizeError(err, 'Failed to run summary batch');
	}
}

async function refreshVectorPreflight(): Promise<boolean> {
	try {
		runtimeVectorPreflight = await vectorPreflight();
		runtimeVectorInstalling = isVectorInstallRunning(runtimeVectorPreflight);
		runtimeVectorError = null;
		if (runtimeVectorPreflight.model_installed && runtimeVectorEnabled) {
			runtimeDetectMessage = 'Vector model is ready.';
		}
		return true;
	} catch (err) {
		runtimeVectorPreflight = null;
		runtimeVectorError = normalizeVectorError(err, 'Failed to fetch vector model status');
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
		runtimeVectorError = normalizeVectorError(err, 'Failed to fetch vector index status');
		return false;
	}
}

async function handleVectorInstallModel() {
	runtimeVectorInstalling = true;
	runtimeVectorError = null;
	try {
		const status = await vectorInstallModel(runtimeVectorModel);
		if (status.state === 'failed') {
			runtimeVectorError = status.message ?? 'Model installation failed.';
			runtimeVectorInstalling = false;
			return;
		}
		runtimeVectorInstalling = status.state === 'installing';
		await refreshVectorPreflight();
	} catch (err) {
		runtimeVectorInstalling = false;
		runtimeVectorError = normalizeVectorError(err, 'Failed to install vector model');
	}
}

async function handleVectorReindex() {
	const preflightOk = await refreshVectorPreflight();
	if (!preflightOk || !runtimeVectorPreflight) return;
	if (!runtimeVectorPreflight.ollama_reachable) {
		runtimeVectorError =
			runtimeVectorPreflight.message ??
			'Ollama is not reachable. Start it with `ollama serve`.';
		return;
	}
	if (!runtimeVectorPreflight.model_installed) {
		runtimeVectorError =
			runtimeVectorPreflight.message ??
			`Model ${runtimeVectorPreflight.model} is not installed. Install model first.`;
		return;
	}

	runtimeVectorReindexing = true;
	runtimeVectorError = null;
	try {
		runtimeVectorIndex = await vectorIndexRebuild();
		runtimeVectorReindexing = isVectorIndexRunning(runtimeVectorIndex);
		if (runtimeVectorIndex?.state === 'failed') {
			runtimeVectorError = runtimeVectorIndex.message ?? 'Vector index rebuild failed.';
		}
	} catch (err) {
		runtimeVectorReindexing = false;
		runtimeVectorError = normalizeVectorError(err, 'Failed to start vector index rebuild');
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
		error = normalizeError(err, 'Failed to issue API key');
	} finally {
		issuing = false;
	}
}

async function copyApiKey() {
	if (!issuedApiKey) return;
	try {
		await navigator.clipboard.writeText(issuedApiKey);
		copyMessage = 'Copied';
	} catch {
		copyMessage = 'Copy failed';
	}
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
		credentialsError = normalizeError(err, 'Failed to save git credential');
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
		credentialsError = normalizeError(err, 'Failed to delete git credential');
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
	const hasActiveBackgroundJob =
		runtimeVectorInstalling ||
		isVectorInstallRunning(runtimeVectorPreflight) ||
		runtimeVectorReindexing ||
		isVectorIndexRunning(runtimeVectorIndex) ||
		runtimeSummaryBatchRunning ||
		isSummaryBatchRunning(runtimeSummaryBatchStatus) ||
		isLifecycleCleanupRunning(runtimeLifecycleStatus);
	const shouldPoll = runtimeSupported && (hasActiveBackgroundJob || runtimeLifecycleEnabled);
	if (!shouldPoll) {
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
		timer = window.setTimeout(
			poll,
			hasActiveBackgroundJob
				? BACKGROUND_JOB_POLL_INTERVAL_MS
				: BACKGROUND_STATUS_POLL_INTERVAL_MS,
		);
	};

	timer = window.setTimeout(
		poll,
		hasActiveBackgroundJob
			? BACKGROUND_JOB_POLL_INTERVAL_MS
			: BACKGROUND_STATUS_POLL_INTERVAL_MS,
	);

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
	<title>Settings - opensession.io</title>
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
		<p class="text-[11px] uppercase tracking-[0.12em] text-text-muted">Account</p>
		<h1 class="mt-1 text-3xl font-semibold tracking-tight text-text-primary">Settings</h1>
		<p class="mt-1 text-sm text-text-secondary">Personal profile and API access controls.</p>
	</header>

		{#if loading}
			<div class="border border-border bg-bg-secondary px-4 py-8 text-center text-sm text-text-muted">Loading...</div>
		{:else if authApiEnabled && authRequired}
			<section
				id="settings-section-profile"
				data-testid="settings-require-auth"
				class="scroll-mt-24 border border-border bg-bg-secondary px-4 py-6 text-sm text-text-secondary xl:max-w-3xl"
			>
			<p class="text-text-primary">Sign in is required to view personal settings.</p>
			<div class="mt-4">
				<button
					type="button"
					onclick={() => onNavigate('/login')}
					class="bg-accent px-3 py-2 text-xs font-semibold text-white hover:bg-accent/85"
				>
					Go to login
				</button>
			</div>
		</section>
	{:else if authApiEnabled}
		<section id="settings-section-profile" class="scroll-mt-24 border border-border bg-bg-secondary p-4 xl:max-w-3xl">
			<h2 class="text-sm font-semibold text-text-primary">Profile</h2>
			{#if settings}
				<dl class="mt-3 grid gap-2 text-xs text-text-secondary sm:grid-cols-[10rem_1fr]">
					<dt>User ID</dt>
					<dd class="font-mono text-text-primary">{settings.user_id}</dd>
					<dt>Nickname</dt>
					<dd class="text-text-primary">{settings.nickname}</dd>
					<dt>Email</dt>
					<dd class="text-text-primary">{settings.email ?? 'not linked'}</dd>
					<dt>Joined</dt>
					<dd class="text-text-primary">{formatDate(settings.created_at)}</dd>
					<dt>Linked OAuth</dt>
					<dd class="text-text-primary">
						{#if settings.oauth_providers.length === 0}
							none
						{:else}
							{settings.oauth_providers.map((provider) => provider.display_name).join(', ')}
						{/if}
					</dd>
				</dl>
			{:else}
				<p class="mt-2 text-xs text-text-muted">No profile data available.</p>
			{/if}
		</section>

		<section id="settings-section-api-key" class="scroll-mt-24 border border-border bg-bg-secondary p-4 xl:max-w-3xl">
			<div class="flex flex-wrap items-center justify-between gap-3">
				<div>
					<h2 class="text-sm font-semibold text-text-primary">Personal API Key</h2>
					<p class="mt-1 text-xs text-text-secondary">
						Issue a new key for CLI and automation access. Existing active key moves to grace mode.
					</p>
				</div>
				<button
					type="button"
					data-testid="issue-api-key-button"
					onclick={handleIssueApiKey}
					disabled={issuing}
					class="bg-accent px-3 py-2 text-xs font-semibold text-white hover:bg-accent/85 disabled:opacity-60"
				>
					{issuing ? 'Issuing...' : issuedApiKey ? 'Regenerate key' : 'Issue key'}
				</button>
			</div>

			{#if issuedApiKey}
				<div class="mt-4 border border-border/80 bg-bg-primary p-3">
					<p class="mb-2 text-xs text-text-muted">Shown once. Save this key now.</p>
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
							Copy
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
				<h2 class="text-sm font-semibold text-text-primary">Private Git Credentials</h2>
				<p class="text-xs text-text-secondary">
					Preferred: connect GitHub/GitLab OAuth. Manual credentials are used for private self-managed or generic git remotes.
				</p>
			</div>

			{#if !credentialsSupported}
				<p class="mt-3 text-xs text-text-muted">
					This deployment does not expose credential management endpoints.
				</p>
			{:else}
				<div class="mt-4 space-y-3">
					<div class="grid gap-2 sm:grid-cols-2">
						<input
							data-testid="git-credential-label"
							type="text"
							placeholder="Label"
							bind:value={credentialLabel}
							class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
						/>
						<input
							data-testid="git-credential-host"
							type="text"
							placeholder="Host (e.g. gitlab.internal.example.com)"
							bind:value={credentialHost}
							class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
						/>
						<input
							data-testid="git-credential-path-prefix"
							type="text"
							placeholder="Path prefix (optional, e.g. group/subgroup)"
							bind:value={credentialPathPrefix}
							class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
						/>
						<input
							data-testid="git-credential-header-name"
							type="text"
							placeholder="Header name"
							bind:value={credentialHeaderName}
							class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
						/>
						<input
							data-testid="git-credential-header-value"
							type="password"
							placeholder="Header value (secret)"
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
							{creatingCredential ? 'Saving...' : 'Save credential'}
						</button>
					</div>
				</div>

				<div class="mt-4 border border-border/70">
					<div class="grid grid-cols-[1.1fr_1fr_1fr_auto] gap-2 border-b border-border bg-bg-primary px-3 py-2 text-[11px] uppercase tracking-[0.08em] text-text-muted">
						<span>Label</span>
						<span>Host</span>
						<span>Path Prefix</span>
						<span>Action</span>
					</div>
					{#if credentialsLoading}
						<div class="px-3 py-3 text-xs text-text-muted">Loading credentials...</div>
					{:else if credentials.length === 0}
						<div class="px-3 py-3 text-xs text-text-muted">No manual credentials registered.</div>
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
									{deletingCredentialId === credential.id ? 'Deleting...' : 'Delete'}
								</button>
							</div>
						{/each}
					{/if}
				</div>
				<p class="mt-2 text-[11px] text-text-muted">
					Secrets are never shown again after save. Stored values are encrypted at rest.
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
				<h2 class="text-sm font-semibold text-text-primary">Runtime Summary (Desktop)</h2>
				<p class="mt-1 text-xs text-text-secondary">
					Provider, prompt, response, and storage settings for desktop local runtime.
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
						{runtimeDetecting ? 'Detecting...' : 'Detect Provider'}
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
			<p class="mt-3 text-xs text-text-muted">Loading runtime settings...</p>
		{:else if !runtimeSupported}
			<p class="mt-3 text-xs text-text-muted">
				Runtime settings are not available in this environment (desktop IPC required).
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
						label="Default Session View"
						help={runtimeHelp.defaultSessionView}
						testId="runtime-help-default-session-view"
					/>
					<select bind:value={runtimeSessionDefaultView} class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary">
						<option value="full">full</option>
						<option value="compressed">compressed</option>
					</select>
				</label>

				<section
					id="runtime-section-provider"
					class="scroll-mt-24 space-y-2 border border-border/60 p-3"
					data-testid="settings-runtime-provider"
				>
					<h3 class="text-xs font-semibold uppercase tracking-[0.08em] text-text-muted">Provider</h3>
					<label class="block text-xs text-text-secondary">
						<FieldHelp
							label="Summary Provider"
							help={runtimeHelp.summaryProvider}
							testId="runtime-help-summary-provider"
						/>
						<select
							bind:value={runtimeProvider}
							onchange={handleRuntimeProviderChange}
							data-testid="runtime-provider-select"
							class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
						>
							<option value="disabled">disabled</option>
							<option value="ollama">ollama</option>
							<option value="codex_exec">codex_exec</option>
							<option value="claude_cli">claude_cli</option>
						</select>
					</label>
					<p class="text-[11px] text-text-muted" data-testid="runtime-provider-transport">
						transport: {currentRuntimeProviderTransport()}
					</p>
					{#if currentRuntimeProviderTransport() === 'http'}
						<label class="block text-xs text-text-secondary">
							<FieldHelp
								label="Endpoint"
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
								label="Model"
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
								label="Model (optional)"
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
							{runtimeDetectMessage ?? 'Run Detect Provider to verify CLI availability.'}
						</p>
					{/if}
				</section>

				<section
					id="runtime-section-prompt"
					class="scroll-mt-24 space-y-2 border border-border/60 p-3"
					data-testid="settings-runtime-prompt"
				>
					<h3 class="text-xs font-semibold uppercase tracking-[0.08em] text-text-muted">Prompt</h3>
					<label class="block text-xs text-text-secondary">
						<FieldHelp
							label="Prompt Template"
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
							Reset to default
						</button>
					</div>
					<label class="block text-xs text-text-secondary">
						<span class="mb-1 block">Default Template (read-only)</span>
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
					<h3 class="text-xs font-semibold uppercase tracking-[0.08em] text-text-muted">Response</h3>
					<div class="grid gap-2 sm:grid-cols-2">
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label="Response Style"
								help={runtimeHelp.responseStyle}
								testId="runtime-help-response-style"
							/>
							<select bind:value={runtimeResponseStyle} class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary">
								<option value="compact">compact</option>
								<option value="standard">standard</option>
								<option value="detailed">detailed</option>
							</select>
						</label>
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label="Output Shape"
								help={runtimeHelp.outputShape}
								testId="runtime-help-output-shape"
							/>
							<select bind:value={runtimeOutputShape} class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary">
								<option value="layered">layered</option>
								<option value="file_list">file_list</option>
								<option value="security_first">security_first</option>
							</select>
						</label>
					</div>
					<p class="text-[11px] text-text-muted">
						Desktop source mode is locked to <code>session_only</code> ({runtimeSourceMode}).
					</p>
					<div class="border border-border/70 bg-bg-primary p-2" data-testid="settings-response-preview">
						<p class="mb-2 text-[11px] uppercase tracking-[0.08em] text-text-muted">Response Preview</p>
						<pre class="max-w-full whitespace-pre-wrap text-xs text-text-secondary [overflow-wrap:anywhere]">{responsePreview(runtimeResponseStyle, runtimeOutputShape)}</pre>
					</div>
				</section>

				<section
					id="runtime-section-vector"
					class="scroll-mt-24 space-y-2 border border-border/60 p-3"
					data-testid="settings-runtime-vector"
				>
					<h3 class="text-xs font-semibold uppercase tracking-[0.08em] text-text-muted">Vector Search</h3>
					<div class="grid gap-2 sm:grid-cols-2">
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label="Model"
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
								label="Endpoint"
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
								label="Chunking Mode"
								help={runtimeHelp.vectorChunkingMode}
								testId="runtime-help-vector-chunking-mode"
							/>
							<select
								bind:value={runtimeVectorChunkingMode}
								data-testid="runtime-vector-chunking-mode"
								class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
							>
								<option value="auto">auto</option>
								<option value="manual">manual</option>
							</select>
						</label>
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label="Chunk Size (lines)"
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
								label="Chunk Overlap (lines)"
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
								label="Top K Chunks"
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
								label="Top K Sessions"
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
							label="Enable semantic vector search"
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
							{runtimeVectorInstalling ? 'Installing...' : 'Install model'}
						</button>
						<button
							type="button"
							data-testid="runtime-vector-reindex"
							onclick={handleVectorReindex}
							disabled={runtimeVectorReindexing || runtimeSaving || runtimeLoading || !runtimeVectorPreflight?.ollama_reachable || !runtimeVectorPreflight?.model_installed}
							class="border border-border px-2 py-1 text-xs text-text-secondary hover:text-text-primary disabled:opacity-60"
						>
							{runtimeVectorReindexing ? 'Reindexing...' : 'Rebuild index'}
						</button>
					</div>

					<div class="rounded border border-border/60 bg-bg-primary px-3 py-3 text-sm text-text-muted" data-testid="runtime-vector-status">
						<p class="font-medium text-text-primary">
							Provider {runtimeVectorProvider} · granularity {runtimeVectorGranularity} · chunking {runtimeVectorChunkingMode}
						</p>
						{#if runtimeVectorPreflight}
							<p class="mt-1">
								Model {runtimeVectorPreflight.model} · reachable {runtimeVectorPreflight.ollama_reachable ? 'yes' : 'no'} · installed{' '}
								{runtimeVectorPreflight.model_installed ? 'yes' : 'no'} · install {runtimeVectorPreflight.install_state}
								({runtimeVectorPreflight.progress_pct}%)
							</p>
							{#if runtimeVectorPreflight.message}
								<p class="mt-1">{runtimeVectorPreflight.message}</p>
							{/if}
						{:else}
							<p class="mt-1">Vector model status unavailable.</p>
						{/if}
						{#if runtimeVectorIndex}
							<p class="mt-1">
								Index {runtimeVectorIndex.state} · processed {runtimeVectorIndex.processed_sessions}/{runtimeVectorIndex.total_sessions}
								{#if progressPercent(runtimeVectorIndex.processed_sessions, runtimeVectorIndex.total_sessions) != null}
									· {progressPercent(runtimeVectorIndex.processed_sessions, runtimeVectorIndex.total_sessions)}%
								{/if}
							</p>
							{#if progressPercent(runtimeVectorIndex.processed_sessions, runtimeVectorIndex.total_sessions) != null}
								<div class="mt-2" data-testid="runtime-vector-progress">
									<div class="flex items-center justify-between text-[11px] text-text-secondary">
										<span>Embedding progress</span>
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
										? `Last rebuild failure:\n${runtimeVectorIndex.message}`
										: runtimeVectorIndex.message}
								</p>
								{#if runtimeVectorIndex.state === 'failed' && runtimeVectorPreflight?.ollama_reachable && runtimeVectorPreflight?.model_installed}
									<p class="mt-1 text-[11px] text-text-secondary">
										Provider looks reachable now. Click <strong>Rebuild index</strong> to retry with the current endpoint.
									</p>
								{/if}
							{/if}
							{#if runtimeVectorIndex.started_at || runtimeVectorIndex.finished_at}
								<p class="mt-1 text-[11px] text-text-secondary">
									started: {formatDate(runtimeVectorIndex.started_at)} | finished: {formatDate(runtimeVectorIndex.finished_at)}
								</p>
							{/if}
						{/if}
						<div class="mt-2 space-y-1 rounded border border-border/50 bg-bg-secondary/50 px-2 py-2 text-[11px] text-text-secondary">
							<p class="font-semibold uppercase tracking-[0.08em] text-text-muted">Actions</p>
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
					<h3 class="text-xs font-semibold uppercase tracking-[0.08em] text-text-muted">Change Reader</h3>
					<div
						class="space-y-1 rounded border border-border/60 bg-bg-primary px-3 py-2 text-[11px] text-text-secondary"
						data-testid="runtime-change-reader-mode-guide"
					>
						<p><span class="font-semibold text-text-primary">Reader</span>: text view of the selected change context.</p>
						<p><span class="font-semibold text-text-primary">Follow-up Q&amp;A</span>: ask extra text questions about that same change context.</p>
						<p><span class="font-semibold text-text-primary">Voice playback</span>: read the change reader output aloud with TTS. Requires a Voice API key.</p>
					</div>
					<label class="flex items-center gap-2 text-xs text-text-secondary">
						<input
							type="checkbox"
							bind:checked={runtimeChangeReaderEnabled}
							data-testid="runtime-change-reader-enable"
						/>
						<FieldHelp
							inline
							label="Enable notebook-style change reading"
							help={runtimeHelp.changeReaderEnable}
							testId="runtime-help-change-reader-enable"
						/>
					</label>
					<div class="grid gap-2 sm:grid-cols-2">
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label="Default Scope"
								help={runtimeHelp.changeReaderScope}
								testId="runtime-help-change-reader-scope"
							/>
							<select bind:value={runtimeChangeReaderScope} class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary">
								<option value="summary_only">summary_only</option>
								<option value="full_context">full_context</option>
							</select>
						</label>
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label="Max Context Chars"
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
							label="Enable Follow-up Q&A"
							help={runtimeHelp.changeReaderQa}
							testId="runtime-help-change-reader-qa"
						/>
					</label>
					<div class="space-y-2 rounded border border-border/60 bg-bg-primary px-2 py-2">
						<p class="text-[11px] font-semibold uppercase tracking-[0.08em] text-text-muted">Voice Playback</p>
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
								label="Enable Voice Playback (TTS)"
								help={runtimeHelp.changeReaderVoiceEnable}
								testId="runtime-help-change-reader-voice-enable"
							/>
						</label>
						<div class="grid gap-2 sm:grid-cols-3">
							<label class="text-xs text-text-secondary">
								<FieldHelp
									label="Voice Provider"
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
									label="Voice Model"
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
									label="Voice Name"
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
								label="Voice API Key (write-only)"
								help={runtimeHelp.changeReaderVoiceApiKey}
								testId="runtime-help-change-reader-voice-api-key"
							/>
							<input
								type="password"
								placeholder={runtimeChangeReaderVoiceApiKeyConfigured ? 'Configured (enter new key to rotate)' : 'Enter API key'}
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
						Uses the configured summary provider when available, then falls back to local heuristic context extraction.
					</p>
				</section>

				<section
					id="runtime-section-storage"
					class="scroll-mt-24 space-y-2 border border-border/60 p-3"
					data-testid="settings-runtime-storage"
				>
					<h3 class="text-xs font-semibold uppercase tracking-[0.08em] text-text-muted">Storage</h3>
					<div class="grid gap-2 sm:grid-cols-2">
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label="Trigger"
								help={runtimeHelp.storageTrigger}
								testId="runtime-help-storage-trigger"
							/>
							<select bind:value={runtimeTriggerMode} class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary">
								<option value="manual">manual</option>
								<option value="on_session_save">on_session_save</option>
							</select>
						</label>
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label="Backend"
								help={runtimeHelp.storageBackend}
								testId="runtime-help-storage-backend"
							/>
							<select bind:value={runtimeStorageBackend} class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary">
								<option value="hidden_ref">hidden_ref (git refs)</option>
								<option value="local_db">local_db (sqlite)</option>
								<option value="none">none (no persistence)</option>
							</select>
						</label>
					</div>
					<div class="space-y-1 rounded border border-border/60 bg-bg-primary px-2 py-2 text-[11px] text-text-muted" data-testid="runtime-storage-backend-notice">
						<p>
							Current persisted backend:
							{#if persistedStorageBackend()}
								<code>{persistedStorageBackend()}</code> ({persistedStorageBackendLabel()})
							{:else}
								unknown
							{/if}
						</p>
						<p>
							Selected backend:
							<code>{runtimeStorageBackend}</code> ({storageBackendLabel(runtimeStorageBackend)})
						</p>
						<p>{storageBackendSummary(runtimeStorageBackend)}</p>
						<p>{storageBackendDetails(runtimeStorageBackend)}</p>
						<p data-testid="runtime-storage-transition-note">{storageBackendTransitionDetail()}</p>
						{#if hasPendingStorageBackendChange()}
							<p class="text-text-primary">Apply this change with <strong>{runtimeSaveLabel()}</strong> above.</p>
						{/if}
					</div>
				</section>

				<section
					id="runtime-section-summary-batch"
					class="scroll-mt-24 space-y-2 border border-border/60 p-3"
					data-testid="settings-runtime-summary-batch"
				>
					<div class="flex flex-wrap items-center justify-between gap-2">
						<h3 class="text-xs font-semibold uppercase tracking-[0.08em] text-text-muted">Summary Batch</h3>
						<button
							type="button"
							data-testid="runtime-summary-batch-run"
							onclick={handleRunSummaryBatchNow}
							disabled={runtimeSummaryBatchRunning || runtimeSaving || runtimeLoading}
							class="inline-flex h-9 items-center border border-border px-3 text-xs text-text-secondary hover:text-text-primary disabled:opacity-60"
						>
							{runtimeSummaryBatchRunning ? 'Running...' : 'Run now'}
						</button>
					</div>
					<div class="grid gap-2 sm:grid-cols-3">
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label="Execution Mode"
								help={runtimeHelp.batchExecution}
								testId="runtime-help-batch-execution-mode"
							/>
							<select bind:value={runtimeBatchExecutionMode} class="h-9 w-full border border-border bg-bg-primary px-2 text-xs text-text-primary">
								<option value="manual">manual</option>
								<option value="on_app_start">on_app_start</option>
							</select>
						</label>
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label="Scope"
								help={runtimeHelp.batchScope}
								testId="runtime-help-batch-scope"
							/>
							<select bind:value={runtimeBatchScope} class="h-9 w-full border border-border bg-bg-primary px-2 text-xs text-text-primary">
								<option value="recent_days">recent_days</option>
								<option value="all">all</option>
							</select>
						</label>
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label="Recent Days"
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
								state: {runtimeSummaryBatchStatus.state} | {summaryBatchProgressLabel(runtimeSummaryBatchStatus)}
							</p>
							{#if progressPercent(runtimeSummaryBatchStatus.processed_sessions, runtimeSummaryBatchStatus.total_sessions) != null}
								<div class="mt-2" data-testid="runtime-summary-batch-progress">
									<div class="flex items-center justify-between text-[11px] text-text-secondary">
										<span>Batch progress</span>
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
								started: {formatDate(runtimeSummaryBatchStatus.started_at)} | finished: {formatDate(runtimeSummaryBatchStatus.finished_at)}
							</p>
						{:else}
							<p>batch status unavailable.</p>
						{/if}
					</div>
				</section>

				<section
					id="runtime-section-lifecycle"
					class="scroll-mt-24 space-y-2 border border-border/60 p-3"
					data-testid="settings-runtime-lifecycle"
				>
					<h3 class="text-xs font-semibold uppercase tracking-[0.08em] text-text-muted">Data Lifecycle</h3>
					<label class="flex items-center gap-2 text-xs text-text-secondary">
						<input type="checkbox" bind:checked={runtimeLifecycleEnabled} data-testid="runtime-lifecycle-enable" />
						<FieldHelp
							inline
							label="Enable periodic lifecycle cleanup"
							help={runtimeHelp.lifecycleEnable}
							testId="runtime-help-lifecycle-enable"
						/>
					</label>
					<div class="grid gap-2 sm:grid-cols-3">
						<label class="text-xs text-text-secondary">
							<FieldHelp
								label="Session TTL (days)"
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
								label="Summary TTL (days)"
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
								label="Cleanup Interval (sec)"
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
								state: {runtimeLifecycleStatus.state} | deleted: {runtimeLifecycleStatus.deleted_sessions} sessions /
								{runtimeLifecycleStatus.deleted_summaries} summaries
							</p>
							<p class="mt-1">
								next run: {lifecycleNextRunLabel()} | interval: {formatIntervalSeconds(runtimeCleanupIntervalSecs)}
							</p>
							{#if runtimeLifecycleStatus.message}
								<p class="mt-1">{runtimeLifecycleStatus.message}</p>
							{/if}
							<p class="mt-1">
								started: {formatDate(runtimeLifecycleStatus.started_at)} | finished: {formatDate(runtimeLifecycleStatus.finished_at)}
							</p>
						{:else}
							<p>No lifecycle cleanup runs recorded yet.</p>
						{/if}
					</div>
					<div class="overflow-x-auto border border-border/70 bg-bg-primary p-2">
						<p class="mb-2 text-[11px] uppercase tracking-[0.08em] text-text-muted">Root / Dependent Cleanup</p>
						<table class="w-full text-left text-[11px] text-text-secondary">
							<thead>
								<tr class="text-text-muted">
									<th class="pb-1">Root</th>
									<th class="pb-1">Dependents</th>
									<th class="pb-1">Rule</th>
								</tr>
							</thead>
							<tbody>
								<tr>
									<td class="pr-4 align-top">session</td>
									<td class="pr-4 align-top">summary, vector chunks, vector index, body cache, session links</td>
									<td class="align-top">delete session => delete all dependents</td>
								</tr>
								<tr>
									<td class="pr-4 align-top">summary</td>
									<td class="pr-4 align-top">summary metadata (hidden_ref/local row)</td>
									<td class="align-top">delete summary => keep session</td>
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
						Reset Draft
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
