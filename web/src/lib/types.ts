// Re-export everything from @opensession/ui
// This shim allows existing $lib/types imports to work
export * from '@opensession/ui';
export type { SessionListItem, UserSettings, InviteInfo, ToolConfig } from '@opensession/ui';
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
} from '@opensession/ui';
