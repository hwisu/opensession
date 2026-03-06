// @opensession/ui — shared types, utilities, and API client

// API client
export {
	ApiError,
	authLogin,
	authLogout,
	authRegister,
	buildSessionHandoff,
	detectSummaryProvider,
	getApiCapabilities,
	getAuthProviders,
	getLocalReviewBundle,
	getOAuthUrl,
	getParsePreviewError,
	getRuntimeSettings,
	getSummaryBatchStatus,
	getSession,
	getSessionDetail,
	getSessionSemanticSummary,
	getSettings,
	handleAuthCallback,
	isAuthApiAvailable,
	isAuthenticated,
	isParsePreviewApiAvailable,
	listSessionRepos,
	listSessions,
	PreviewApiError,
	previewSessionFromGithubSource,
	previewSessionFromGitSource,
	previewSessionFromInlineSource,
	quickShareSession,
	regenerateSessionSemanticSummary,
	runSummaryBatch,
	setBaseUrl,
	updateRuntimeSettings,
	verifyAuth,
} from './api';
// API types (auto-generated)
export type * from './api-types.generated';
// Constants
export * from './constants';
// Event helpers
export {
	calcContentLength,
	findCodeStats,
	findFirstText,
	findJsonPayload,
	firstMeaningfulEventLine,
	formatContentLength,
	getToolName,
	isToolError,
	prepareTimelineEvents,
	truncate,
} from './event-helpers';
// HAIL parser helpers
export { parseHailInput, parseHailJsonl } from './hail-parse';
export {
	buildStateUrl as buildSourceStateUrl,
	buildRouteBase as buildSourceRouteBase,
	parseSourceRoute,
	type SourceRouteState,
} from './source-route';
// Highlight & Markdown utilities
export { highlightCode } from './highlight';
export { isLongContent, lineCount, renderMarkdown } from './markdown';
// Parser conformance constants
export {
	conformanceCoverageScore,
	PARSER_CONFORMANCE_ROWS,
	type ParserConformanceReference,
	type ParserConformanceRow,
	type SourceStatus,
} from './parser-conformance';
// Session filter/view helpers
export {
	branchpointFilterKeyForEvent,
	buildBranchpointFilterOptions,
	buildNativeFilterOptions,
	buildUnifiedFilterOptions,
	type FilterOption,
	filterEventsByBranchpointKeys,
	filterEventsByNativeGroups,
	filterEventsByUnifiedKeys,
	isNativeAdapterSupported,
	nativeGroupForEvent,
	type SessionViewMode,
	unifiedFilterKeyForEvent,
} from './session-filters';
// HAIL core types + UI types
export type {
	Agent,
	ApiErrorResponse,
	Content,
	ContentBlock,
	Event,
	EventType,
	Session,
	SessionContext,
	SessionDetail,
	SessionListResponse,
	SessionSummary,
	Stats,
	ToolConfig,
} from './types';
export { formatDuration, formatTimestamp, getToolConfig, TOOL_CONFIGS } from './types';
// Shared utilities
export type { FileStats } from './utils';
export { computeFileStats, formatFullDate, getDisplayTitle, stripTags } from './utils';
export {
	createShellModel,
	createShellModelState,
	type ShellModelState,
} from './models/app-shell-model';
export {
	createSourcePreviewModel,
	createSourcePreviewModelState,
	type SourcePreviewLocation,
	type SourcePreviewModelState,
} from './models/source-preview-model';
export {
	copyTextSurface,
	loadGitCredentialsState,
	loadRuntimeSettingsState,
	loadSettingsPageState,
	nextSettingsBackgroundPollDelay,
	type RuntimeSettingsLoadResult,
	type SettingsPageLoadResult,
} from './models/settings-model';
