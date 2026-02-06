// ─── HAIL core types (mirrors crates/core/src/trace.rs) ──────────────────────
// These represent the full session trace format used by /api/sessions/:id/raw

export interface Session {
	version: string;
	session_id: string;
	agent: Agent;
	context: SessionContext;
	events: Event[];
	stats: Stats;
}

export interface Agent {
	provider: string;
	model: string;
	tool: string;
	tool_version?: string;
}

export interface SessionContext {
	title?: string;
	description?: string;
	tags: string[];
	created_at: string;
	updated_at: string;
	attributes?: Record<string, unknown>;
}

export interface Event {
	event_id: string;
	timestamp: string;
	event_type: EventType;
	task_id?: string;
	content: Content;
	duration_ms?: number;
	attributes?: Record<string, unknown>;
}

export type EventType =
	| { type: 'UserMessage' }
	| { type: 'AgentMessage' }
	| { type: 'SystemMessage' }
	| { type: 'Thinking' }
	| { type: 'ToolCall'; data: { name: string } }
	| { type: 'ToolResult'; data: { name: string; is_error: boolean; call_id?: string } }
	| { type: 'FileRead'; data: { path: string } }
	| { type: 'CodeSearch'; data: { query: string } }
	| { type: 'FileSearch'; data: { pattern: string } }
	| { type: 'FileEdit'; data: { path: string; diff?: string } }
	| { type: 'FileCreate'; data: { path: string } }
	| { type: 'FileDelete'; data: { path: string } }
	| { type: 'ShellCommand'; data: { command: string; exit_code?: number } }
	| { type: 'ImageGenerate'; data: { prompt: string } }
	| { type: 'VideoGenerate'; data: { prompt: string } }
	| { type: 'AudioGenerate'; data: { prompt: string } }
	| { type: 'WebSearch'; data: { query: string } }
	| { type: 'WebFetch'; data: { url: string } }
	| { type: 'TaskStart'; data: { title?: string } }
	| { type: 'TaskEnd'; data: { summary?: string } }
	| { type: 'Custom'; data: { kind: string } };

export interface Content {
	blocks: ContentBlock[];
}

export type ContentBlock =
	| { type: 'Text'; text: string }
	| { type: 'Code'; code: string; language?: string; start_line?: number }
	| { type: 'Image'; url: string; alt?: string; mime: string }
	| { type: 'Video'; url: string; mime: string }
	| { type: 'Audio'; url: string; mime: string }
	| { type: 'File'; path: string; content?: string }
	| { type: 'Json'; data: unknown }
	| { type: 'Reference'; uri: string; media_type: string };

export interface Stats {
	event_count: number;
	message_count: number;
	tool_call_count: number;
	task_count: number;
	duration_seconds: number;
}

// ─── API types (auto-generated from Rust — single source of truth) ───────────
// See: crates/api-types/src/lib.rs
// Regenerate: cargo test -p opensession-api-types -- export_typescript

export type {
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
	ApiError as ApiErrorResponse
} from './api-types.generated';

// Convenience alias for backward compatibility
export type SessionListItem = import('./api-types.generated').SessionSummary;

// ─── UI-only types (not from server) ─────────────────────────────────────────

export type UserSettings = import('./api-types.generated').UserSettingsResponse;

export interface InviteInfo {
	group_id: string;
	group_name: string;
	group_description?: string;
	member_count: number;
	inviter?: string;
}

export interface ToolConfig {
	name: string;
	label: string;
	color: string;
	icon: string;
}

export const TOOL_CONFIGS: Record<string, ToolConfig> = {
	'claude-code': {
		name: 'claude-code',
		label: 'Claude Code',
		color: 'var(--color-tool-claude)',
		icon: 'C'
	},
	cursor: {
		name: 'cursor',
		label: 'Cursor',
		color: 'var(--color-tool-cursor)',
		icon: 'Cu'
	},
	goose: {
		name: 'goose',
		label: 'Goose',
		color: 'var(--color-tool-goose)',
		icon: 'G'
	},
	windsurf: {
		name: 'windsurf',
		label: 'Windsurf',
		color: 'var(--color-tool-windsurf)',
		icon: 'W'
	},
	copilot: {
		name: 'copilot',
		label: 'Copilot',
		color: 'var(--color-tool-copilot)',
		icon: 'Co'
	},
	codex: {
		name: 'codex',
		label: 'Codex',
		color: 'var(--color-tool-codex)',
		icon: 'Cx'
	},
	opencode: {
		name: 'opencode',
		label: 'OpenCode',
		color: 'var(--color-tool-opencode)',
		icon: 'Oc'
	},
	cline: {
		name: 'cline',
		label: 'Cline',
		color: 'var(--color-tool-cline)',
		icon: 'Cl'
	},
	amp: {
		name: 'amp',
		label: 'Amp',
		color: 'var(--color-tool-amp)',
		icon: 'Ap'
	}
};

export function getToolConfig(tool: string): ToolConfig {
	return (
		TOOL_CONFIGS[tool] ?? {
			name: tool,
			label: tool,
			color: 'var(--color-tool-default)',
			icon: tool.charAt(0).toUpperCase()
		}
	);
}

export function formatDuration(seconds: number): string {
	if (seconds < 60) return `${seconds}s`;
	if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
	const h = Math.floor(seconds / 3600);
	const m = Math.floor((seconds % 3600) / 60);
	return `${h}h ${m}m`;
}

export function formatTimestamp(ts: string): string {
	const date = new Date(ts);
	const now = new Date();
	const diff = now.getTime() - date.getTime();
	const minutes = Math.floor(diff / 60000);
	if (minutes < 1) return 'just now';
	if (minutes < 60) return `${minutes}m ago`;
	const hours = Math.floor(minutes / 60);
	if (hours < 24) return `${hours}h ago`;
	const days = Math.floor(hours / 24);
	if (days < 30) return `${days}d ago`;
	return date.toLocaleDateString();
}
