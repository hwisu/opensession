// @opensession/ui — shared types, utilities, and API client

// API client
export {
	ApiError,
	addMember,
	clearApiKey,
	createTeam,
	getSession,
	getSettings,
	getTeam,
	getTeamStats,
	listMembers,
	listSessions,
	listTeams,
	regenerateApiKey,
	register,
	removeMember,
	setApiKey,
	setBaseUrl,
	updateTeam,
	uploadSession,
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
// Highlight & Markdown utilities
export { highlightCode } from './highlight';
export { isLongContent, lineCount, renderMarkdown } from './markdown';
// HAIL core types + UI types
export type {
	Agent,
	ApiErrorResponse,
	Content,
	ContentBlock,
	Event,
	EventType,
	HealthResponse,
	ListMembersResponse,
	ListTeamsResponse,
	MemberResponse,
	RegisterResponse,
	Session,
	SessionContext,
	SessionDetail,
	SessionListItem,
	SessionListResponse,
	SessionSummary,
	Stats,
	TeamDetailResponse,
	TeamResponse,
	TeamStatsResponse,
	TeamStatsTotals,
	ToolConfig,
	ToolStats,
	UploadResponse,
	UserSettings,
	UserSettingsResponse,
	UserStats,
	VerifyResponse,
} from './types';
export { formatDuration, formatTimestamp, getToolConfig, TOOL_CONFIGS } from './types';
// Shared utilities
export type { FileStats } from './utils';
export { computeFileStats, formatFullDate, getDisplayTitle, stripTags } from './utils';
