import { ApiError } from '../api-internal/errors';
import type {
	DesktopLifecycleCleanupStatusResponse,
	DesktopRuntimeSettingsResponse,
	DesktopSummaryBatchStatusResponse,
	DesktopVectorIndexStatusResponse,
	DesktopVectorPreflightResponse,
	GitCredentialSummary,
	UserSettings,
} from '../types';

export interface SettingsPageLoadDeps {
	getApiCapabilities: () => Promise<{ auth_enabled: boolean }>;
	isAuthenticated: () => boolean;
	getSettings: () => Promise<UserSettings>;
	listGitCredentials: () => Promise<GitCredentialSummary[]>;
}

export interface SettingsPageLoadResult {
	authApiEnabled: boolean;
	authRequired: boolean;
	settings: UserSettings | null;
	error: string | null;
	credentials: GitCredentialSummary[];
	credentialsLoading: boolean;
	credentialsError: string | null;
	credentialsSupported: boolean;
}

export interface RuntimeSettingsLoadDeps {
	getRuntimeSettings: () => Promise<DesktopRuntimeSettingsResponse>;
	getLifecycleCleanupStatus: () => Promise<DesktopLifecycleCleanupStatusResponse>;
	getSummaryBatchStatus: () => Promise<DesktopSummaryBatchStatusResponse>;
	vectorPreflight: () => Promise<DesktopVectorPreflightResponse>;
	vectorIndexStatus: () => Promise<DesktopVectorIndexStatusResponse>;
}

export interface RuntimeSettingsLoadResult {
	runtimeSettings: DesktopRuntimeSettingsResponse | null;
	runtimeSupported: boolean;
	runtimeError: string | null;
	runtimeVectorError: string | null;
	runtimeLifecycleStatus: DesktopLifecycleCleanupStatusResponse | null;
	runtimeSummaryBatchStatus: DesktopSummaryBatchStatusResponse | null;
	runtimeVectorPreflight: DesktopVectorPreflightResponse | null;
	runtimeVectorIndex: DesktopVectorIndexStatusResponse | null;
	runtimeVectorInstalling: boolean;
	runtimeVectorReindexing: boolean;
	runtimeSummaryBatchRunning: boolean;
}

export interface SettingsBackgroundPollState {
	runtimeSupported: boolean;
	runtimeLifecycleEnabled: boolean;
	runtimeVectorInstalling: boolean;
	runtimeVectorPreflight: DesktopVectorPreflightResponse | null;
	runtimeVectorReindexing: boolean;
	runtimeVectorIndex: DesktopVectorIndexStatusResponse | null;
	runtimeSummaryBatchRunning: boolean;
	runtimeSummaryBatchStatus: DesktopSummaryBatchStatusResponse | null;
	runtimeLifecycleStatus: DesktopLifecycleCleanupStatusResponse | null;
}

export interface CopyTextDeps {
	writeText: (text: string) => Promise<void>;
}

function isVectorInstallRunning(status: DesktopVectorPreflightResponse | null): boolean {
	return status?.install_state === 'installing';
}

function isVectorIndexRunning(status: DesktopVectorIndexStatusResponse | null): boolean {
	return status?.state === 'running';
}

function isSummaryBatchRunning(status: DesktopSummaryBatchStatusResponse | null): boolean {
	return status?.state === 'running';
}

function normalizeError(error: unknown, fallback: string): string {
	if (error instanceof ApiError) return error.message || fallback;
	if (error instanceof Error) return error.message || fallback;
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

function normalizeVectorError(error: unknown, fallback: string): string {
	if (error instanceof ApiError) {
		const message = error.message || fallback;
		const hint = apiDetailString(error.details, 'hint');
		const endpoint = apiDetailString(error.details, 'endpoint');
		const reason = apiDetailString(error.details, 'reason');
		const model = apiDetailString(error.details, 'model');
		const status = apiDetailNumber(error.details, 'status');
		const batchReason = apiDetailString(error.details, 'batch_reason');
		const batchEndpoint = apiDetailString(error.details, 'batch_endpoint');
		const batchStatus = apiDetailNumber(error.details, 'batch_status');
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
	return normalizeError(error, fallback);
}

export async function loadGitCredentialsState(
	deps: Pick<SettingsPageLoadDeps, 'listGitCredentials'>,
): Promise<{
	credentials: GitCredentialSummary[];
	credentialsLoading: boolean;
	credentialsError: string | null;
	credentialsSupported: boolean;
}> {
	try {
		return {
			credentials: await deps.listGitCredentials(),
			credentialsLoading: false,
			credentialsError: null,
			credentialsSupported: true,
		};
	} catch (error) {
		if (error instanceof ApiError && error.status === 404) {
			return {
				credentials: [],
				credentialsLoading: false,
				credentialsError: null,
				credentialsSupported: false,
			};
		}
		return {
			credentials: [],
			credentialsLoading: false,
			credentialsError: normalizeError(error, 'Failed to load git credentials'),
			credentialsSupported: true,
		};
	}
}

export async function loadSettingsPageState(
	deps: SettingsPageLoadDeps,
): Promise<SettingsPageLoadResult> {
	let authApiEnabled = false;
	try {
		authApiEnabled = (await deps.getApiCapabilities()).auth_enabled;
	} catch {
		authApiEnabled = false;
	}

	if (!authApiEnabled) {
		return {
			authApiEnabled,
			authRequired: false,
			settings: null,
			error: null,
			credentials: [],
			credentialsLoading: false,
			credentialsError: null,
			credentialsSupported: true,
		};
	}

	if (!deps.isAuthenticated()) {
		return {
			authApiEnabled,
			authRequired: true,
			settings: null,
			error: null,
			credentials: [],
			credentialsLoading: false,
			credentialsError: null,
			credentialsSupported: true,
		};
	}

	try {
		const settings = await deps.getSettings();
		const credentialsState = await loadGitCredentialsState(deps);
		return {
			authApiEnabled,
			authRequired: false,
			settings,
			error: null,
			...credentialsState,
		};
	} catch (error) {
		return {
			authApiEnabled,
			authRequired: error instanceof ApiError && (error.status === 401 || error.status === 403),
			settings: null,
			error:
				error instanceof ApiError && (error.status === 401 || error.status === 403)
					? null
					: normalizeError(error, 'Failed to load settings'),
			credentials: [],
			credentialsLoading: false,
			credentialsError: null,
			credentialsSupported: true,
		};
	}
}

export async function loadRuntimeSettingsState(
	deps: RuntimeSettingsLoadDeps,
): Promise<RuntimeSettingsLoadResult> {
	try {
		const runtimeSettings = await deps.getRuntimeSettings();
		const [lifecycleResult, summaryBatchResult, vectorPreflightResult, vectorIndexResult] =
			await Promise.allSettled([
				deps.getLifecycleCleanupStatus(),
				deps.getSummaryBatchStatus(),
				deps.vectorPreflight(),
				deps.vectorIndexStatus(),
			]);

		const runtimeLifecycleStatus =
			lifecycleResult.status === 'fulfilled' ? lifecycleResult.value : null;
		const runtimeSummaryBatchStatus =
			summaryBatchResult.status === 'fulfilled' ? summaryBatchResult.value : null;
		const runtimeVectorPreflight =
			vectorPreflightResult.status === 'fulfilled' ? vectorPreflightResult.value : null;
		const runtimeVectorIndex =
			vectorIndexResult.status === 'fulfilled' ? vectorIndexResult.value : null;

		const errors = [
			lifecycleResult.status === 'rejected'
				? normalizeError(lifecycleResult.reason, 'Failed to fetch lifecycle cleanup status')
				: null,
			summaryBatchResult.status === 'rejected'
				? normalizeError(summaryBatchResult.reason, 'Failed to fetch summary batch status')
				: null,
		].filter((value): value is string => value != null);

		return {
			runtimeSettings,
			runtimeSupported: true,
			runtimeError: errors[0] ?? null,
			runtimeVectorError:
				vectorPreflightResult.status === 'rejected'
					? normalizeVectorError(
							vectorPreflightResult.reason,
							'Failed to fetch vector model status',
					  )
					: vectorIndexResult.status === 'rejected'
						? normalizeVectorError(
								vectorIndexResult.reason,
								'Failed to fetch vector index status',
						  )
						: null,
			runtimeLifecycleStatus,
			runtimeSummaryBatchStatus,
			runtimeVectorPreflight,
			runtimeVectorIndex,
			runtimeVectorInstalling: isVectorInstallRunning(runtimeVectorPreflight),
			runtimeVectorReindexing: isVectorIndexRunning(runtimeVectorIndex),
			runtimeSummaryBatchRunning: isSummaryBatchRunning(runtimeSummaryBatchStatus),
		};
	} catch (error) {
		return {
			runtimeSettings: null,
			runtimeSupported: !(error instanceof ApiError && error.status === 501),
			runtimeError:
				error instanceof ApiError && error.status === 501
					? null
					: normalizeError(error, 'Failed to load runtime settings'),
			runtimeVectorError: null,
			runtimeLifecycleStatus: null,
			runtimeSummaryBatchStatus: null,
			runtimeVectorPreflight: null,
			runtimeVectorIndex: null,
			runtimeVectorInstalling: false,
			runtimeVectorReindexing: false,
			runtimeSummaryBatchRunning: false,
		};
	}
}

export function hasActiveSettingsBackgroundJob(state: SettingsBackgroundPollState): boolean {
	return (
		state.runtimeVectorInstalling ||
		isVectorInstallRunning(state.runtimeVectorPreflight) ||
		state.runtimeVectorReindexing ||
		isVectorIndexRunning(state.runtimeVectorIndex) ||
		state.runtimeSummaryBatchRunning ||
		isSummaryBatchRunning(state.runtimeSummaryBatchStatus) ||
		state.runtimeLifecycleStatus?.state === 'running'
	);
}

export function nextSettingsBackgroundPollDelay(
	state: SettingsBackgroundPollState,
	activeIntervalMs = 1000,
	statusIntervalMs = 5000,
): number | null {
	if (!state.runtimeSupported) return null;
	const hasActiveJob = hasActiveSettingsBackgroundJob(state);
	if (!hasActiveJob && !state.runtimeLifecycleEnabled) return null;
	return hasActiveJob ? activeIntervalMs : statusIntervalMs;
}

export async function copyTextSurface(
	deps: CopyTextDeps,
	text: string | null | undefined,
): Promise<string> {
	if (!text) return 'Copy failed';
	try {
		await deps.writeText(text);
		return 'Copied';
	} catch {
		return 'Copy failed';
	}
}
