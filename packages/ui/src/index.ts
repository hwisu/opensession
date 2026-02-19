// @opensession/ui — shared types, utilities, and API client

// API client
export {
	ApiError,
	authLogin,
	authLogout,
	authRegister,
	clearApiKey,
	getAuthProviders,
	getOAuthUrl,
	getSession,
	getSettings,
	handleAuthCallback,
	isAuthenticated,
	listSessions,
	setApiKey,
	setBaseUrl,
	uploadSession,
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
	formatContentLength,
	getToolName,
	isToolError,
	truncate,
} from './event-helpers';
// HAIL parser helpers
export { parseHailInput, parseHailJsonl } from './hail-parse';
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
	SessionListItem,
	SessionListResponse,
	SessionSummary,
	Stats,
	ToolConfig,
} from './types';
export { formatDuration, formatTimestamp, getToolConfig, TOOL_CONFIGS } from './types';
// Shared utilities
export type { FileStats } from './utils';
export { computeFileStats, formatFullDate, getDisplayTitle, stripTags } from './utils';
