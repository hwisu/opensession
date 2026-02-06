// Re-export everything from @opensession/ui
// This shim allows existing $lib/types imports to work
export * from '@opensession/ui';
export type { SessionListItem, UserSettings, ToolConfig } from '@opensession/ui';
export type {
	Session,
	Agent,
	SessionContext,
	Event,
	EventType,
	Content,
	ContentBlock,
	Stats,
	SessionSummary,
	SessionListResponse,
	SessionDetail,
	TeamResponse,
	ListTeamsResponse,
	TeamDetailResponse,
	MemberResponse,
	ListMembersResponse,
	RegisterResponse,
	VerifyResponse,
	UserSettingsResponse,
	UploadResponse,
	HealthResponse,
	ApiErrorResponse
} from '@opensession/ui';
