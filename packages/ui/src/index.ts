// @opensession/ui — shared types, utilities, and API client

// HAIL core types + UI types
export type {
	Session,
	Agent,
	SessionContext,
	Event,
	EventType,
	Content,
	ContentBlock,
	Stats,
	SessionListItem,
	UserSettings,
	InviteInfo,
	ToolConfig,
	SessionSummary,
	SessionListResponse,
	SessionDetail,
	GroupRef,
	GroupResponse,
	ListGroupsResponse,
	GroupDetailResponse,
	MemberResponse,
	ListMembersResponse,
	RegisterResponse,
	VerifyResponse,
	UserSettingsResponse,
	UploadResponse,
	InviteResponse,
	JoinResponse,
	HealthResponse,
	ApiErrorResponse
} from './types';
export { TOOL_CONFIGS, getToolConfig, formatDuration, formatTimestamp } from './types';

// API types (auto-generated)
export type * from './api-types.generated';

// Highlight & Markdown utilities
export { highlightCode } from './highlight';
export { renderMarkdown, lineCount, isLongContent } from './markdown';

// API client
export {
	setApiKey,
	clearApiKey,
	setBaseUrl,
	ApiError,
	listSessions,
	getSession,
	uploadSession,
	listGroups,
	getGroup,
	createGroup,
	getInviteInfo,
	joinInvite,
	register,
	getSettings,
	regenerateApiKey
} from './api';
