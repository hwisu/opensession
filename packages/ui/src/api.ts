import type {
	AuthProvidersResponse,
	AuthTokenResponse,
	CapabilitiesResponse,
	DesktopChangeQuestionResponse,
	DesktopChangeReaderScope,
	DesktopChangeReadResponse,
	DesktopChangeReaderTtsResponse,
	DesktopHandoffBuildResponse,
	DesktopLifecycleCleanupStatusResponse,
	DesktopQuickShareResponse,
	DesktopRuntimeSettingsResponse,
	DesktopRuntimeSettingsUpdateRequest,
	DesktopSummaryBatchStatusResponse,
	DesktopSessionSummaryResponse,
	DesktopSummaryProviderDetectResponse,
	DesktopVectorIndexStatusResponse,
	DesktopVectorInstallStatusResponse,
	DesktopVectorPreflightResponse,
	DesktopVectorSearchResponse,
	GitCredentialSummary,
	IssueApiKeyResponse,
	LocalReviewBundle,
	ParsePreviewErrorResponse,
	ParsePreviewResponse,
	Session,
	SessionDetail,
	SessionListResponse,
	SessionRepoListResponse,
	UserSettings,
} from './types';
import type { SessionListParams } from './session-adapter';
import {
	ApiError,
	PreviewApiError,
} from './api-internal/errors';
import {
	authLoginEffect,
	authLogoutEffect,
	authRegisterEffect,
	createGitCredentialEffect,
	deleteGitCredentialEffect,
	getApiCapabilitiesSafeEffect,
	getAuthProvidersSafeEffect,
	getSettingsEffect,
	handleAuthCallbackEffect,
	issueApiKeyEffect,
	isAuthenticatedEffect,
	listGitCredentialsEffect,
	verifyAuthEffect,
} from './api-internal/auth-services';
import {
	getParsePreviewError,
	previewSessionFromGithubSourceEffect,
	previewSessionFromGitSourceEffect,
	previewSessionFromInlineSourceEffect,
} from './api-internal/parse-preview-services';
import { requestEffect } from './api-internal/requests';
import {
	askSessionChangesEffect,
	buildSessionHandoffEffect,
	changeReaderTextToSpeechEffect,
	detectSummaryProviderEffect,
	getLifecycleCleanupStatusEffect,
	getRuntimeSettingsEffect,
	getSessionDetailEffect,
	getSessionEffect,
	getSessionSemanticSummaryEffect,
	getSummaryBatchStatusEffect,
	listSessionReposEffect,
	listSessionsEffect,
	quickShareSessionEffect,
	readSessionChangesEffect,
	regenerateSessionSemanticSummaryEffect,
	runSummaryBatchEffect,
	searchSessionsVectorEffect,
	updateRuntimeSettingsEffect,
	vectorIndexRebuildEffect,
	vectorIndexStatusEffect,
	vectorInstallModelEffect,
	vectorPreflightEffect,
} from './api-internal/session-services';
import {
	getOAuthUrl as getOAuthUrlFromRuntime,
	isAuthenticated as isAuthenticatedInRuntime,
	readBrowserRuntime,
	runUiEffect,
	setBaseUrl as setBaseUrlInRuntime,
} from './api-internal/runtime';

export { ApiError, PreviewApiError, getParsePreviewError };

export function setBaseUrl(url: string) {
	setBaseUrlInRuntime(readBrowserRuntime(), url);
}

export function isAuthenticated(): boolean {
	return isAuthenticatedInRuntime(readBrowserRuntime());
}

export async function verifyAuth(): Promise<boolean> {
	return runUiEffect(verifyAuthEffect());
}

export async function listSessions(params?: SessionListParams): Promise<SessionListResponse> {
	return runUiEffect(listSessionsEffect(params));
}

export async function listSessionRepos(): Promise<SessionRepoListResponse> {
	const repos = await runUiEffect(listSessionReposEffect());
	return { repos };
}

export async function getSession(id: string): Promise<Session> {
	return runUiEffect(getSessionEffect(id));
}

export async function getSessionDetail(id: string): Promise<SessionDetail> {
	return runUiEffect(getSessionDetailEffect(id));
}

export async function getSessionSemanticSummary(
	sessionId: string,
): Promise<DesktopSessionSummaryResponse> {
	return runUiEffect(getSessionSemanticSummaryEffect(sessionId));
}

export async function regenerateSessionSemanticSummary(
	sessionId: string,
): Promise<DesktopSessionSummaryResponse> {
	return runUiEffect(regenerateSessionSemanticSummaryEffect(sessionId));
}

export async function buildSessionHandoff(
	sessionId: string,
	pinLatest: boolean = true,
): Promise<DesktopHandoffBuildResponse> {
	return runUiEffect(buildSessionHandoffEffect(sessionId, pinLatest));
}

export async function quickShareSession(
	sessionId: string,
	remote?: string | null,
): Promise<DesktopQuickShareResponse> {
	return runUiEffect(quickShareSessionEffect(sessionId, remote ?? null));
}

export async function readSessionChanges(
	sessionId: string,
	scope?: DesktopChangeReaderScope | null,
): Promise<DesktopChangeReadResponse> {
	return runUiEffect(readSessionChangesEffect(sessionId, scope));
}

export async function askSessionChanges(
	sessionId: string,
	question: string,
	scope?: DesktopChangeReaderScope | null,
): Promise<DesktopChangeQuestionResponse> {
	return runUiEffect(askSessionChangesEffect(sessionId, question, scope));
}

export async function changeReaderTextToSpeech(
	text: string,
	sessionId?: string | null,
	scope?: DesktopChangeReaderScope | null,
): Promise<DesktopChangeReaderTtsResponse> {
	return runUiEffect(changeReaderTextToSpeechEffect(text, sessionId, scope));
}

export async function getRuntimeSettings(): Promise<DesktopRuntimeSettingsResponse> {
	return runUiEffect(getRuntimeSettingsEffect());
}

export async function updateRuntimeSettings(
	request: DesktopRuntimeSettingsUpdateRequest,
): Promise<DesktopRuntimeSettingsResponse> {
	return runUiEffect(updateRuntimeSettingsEffect(request));
}

export async function getLifecycleCleanupStatus(): Promise<DesktopLifecycleCleanupStatusResponse> {
	return runUiEffect(getLifecycleCleanupStatusEffect());
}

export async function runSummaryBatch(): Promise<DesktopSummaryBatchStatusResponse> {
	return runUiEffect(runSummaryBatchEffect());
}

export async function getSummaryBatchStatus(): Promise<DesktopSummaryBatchStatusResponse> {
	return runUiEffect(getSummaryBatchStatusEffect());
}

export async function detectSummaryProvider(): Promise<DesktopSummaryProviderDetectResponse> {
	return runUiEffect(detectSummaryProviderEffect());
}

export async function vectorPreflight(): Promise<DesktopVectorPreflightResponse> {
	return runUiEffect(vectorPreflightEffect());
}

export async function vectorInstallModel(
	model: string,
): Promise<DesktopVectorInstallStatusResponse> {
	return runUiEffect(vectorInstallModelEffect(model));
}

export async function vectorIndexRebuild(): Promise<DesktopVectorIndexStatusResponse> {
	return runUiEffect(vectorIndexRebuildEffect());
}

export async function vectorIndexStatus(): Promise<DesktopVectorIndexStatusResponse> {
	return runUiEffect(vectorIndexStatusEffect());
}

export async function searchSessionsVector(
	query: string,
	cursor?: string | null,
	limit?: number,
): Promise<DesktopVectorSearchResponse> {
	return runUiEffect(searchSessionsVectorEffect(query, cursor, limit));
}

export async function getLocalReviewBundle(reviewId: string): Promise<LocalReviewBundle> {
	return runUiEffect(requestEffect<LocalReviewBundle>(`/api/review/local/${encodeURIComponent(reviewId)}`));
}

export async function getSettings(): Promise<UserSettings> {
	return runUiEffect(getSettingsEffect());
}

export async function issueApiKey(): Promise<IssueApiKeyResponse> {
	return runUiEffect(issueApiKeyEffect());
}

export async function listGitCredentials(): Promise<GitCredentialSummary[]> {
	return runUiEffect(listGitCredentialsEffect());
}

export async function createGitCredential(params: {
	label: string;
	host: string;
	path_prefix?: string | null;
	header_name: string;
	header_value: string;
}): Promise<GitCredentialSummary> {
	return runUiEffect(createGitCredentialEffect(params));
}

export async function deleteGitCredential(id: string): Promise<void> {
	return runUiEffect(deleteGitCredentialEffect(id));
}

export async function authRegister(
	email: string,
	password: string,
	nickname: string,
): Promise<AuthTokenResponse> {
	return runUiEffect(authRegisterEffect(email, password, nickname));
}

export async function authLogin(email: string, password: string): Promise<AuthTokenResponse> {
	return runUiEffect(authLoginEffect(email, password));
}

export async function authLogout(): Promise<void> {
	return runUiEffect(authLogoutEffect());
}

export async function getAuthProviders(): Promise<AuthProvidersResponse> {
	return runUiEffect(getAuthProvidersSafeEffect());
}

export async function getApiCapabilities(): Promise<CapabilitiesResponse> {
	return runUiEffect(getApiCapabilitiesSafeEffect());
}

export async function isAuthApiAvailable(): Promise<boolean> {
	const capabilities = await getApiCapabilities();
	return capabilities.auth_enabled;
}

export async function isParsePreviewApiAvailable(): Promise<boolean> {
	const capabilities = await getApiCapabilities();
	return capabilities.parse_preview_enabled;
}

export async function previewSessionFromGithubSource(params: {
	owner: string;
	repo: string;
	ref: string;
	path: string;
	parser_hint?: string;
}): Promise<ParsePreviewResponse> {
	return runUiEffect(previewSessionFromGithubSourceEffect(params));
}

export async function previewSessionFromGitSource(params: {
	remote: string;
	ref: string;
	path: string;
	parser_hint?: string;
}): Promise<ParsePreviewResponse> {
	return runUiEffect(previewSessionFromGitSourceEffect(params));
}

export async function previewSessionFromInlineSource(params: {
	filename: string;
	content_base64: string;
	parser_hint?: string;
}): Promise<ParsePreviewResponse> {
	return runUiEffect(previewSessionFromInlineSourceEffect(params));
}

export function getOAuthUrl(provider: string): string {
	return getOAuthUrlFromRuntime(readBrowserRuntime(), provider);
}

export async function handleAuthCallback(): Promise<boolean> {
	return runUiEffect(handleAuthCallbackEffect());
}
