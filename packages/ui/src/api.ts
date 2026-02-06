import type {
	Session,
	SessionListResponse,
	TeamResponse,
	TeamDetailResponse,
	ListTeamsResponse,
	ListMembersResponse,
	MemberResponse,
	UserSettings,
} from './types';

function getBaseUrl(): string {
	if (typeof window !== 'undefined') {
		const stored = localStorage.getItem('opensession_api_url');
		if (stored) return stored;
		// Default: use current origin (works when server serves both API and frontend)
		return window.location.origin;
	}
	return '';
}

function getApiKey(): string | null {
	if (typeof window !== 'undefined') {
		return localStorage.getItem('opensession_api_key');
	}
	return null;
}

export function setApiKey(key: string) {
	localStorage.setItem('opensession_api_key', key);
}

export function clearApiKey() {
	localStorage.removeItem('opensession_api_key');
}

export function setBaseUrl(url: string) {
	localStorage.setItem('opensession_api_url', url);
}

async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
	const url = `${getBaseUrl()}${path}`;
	const headers: Record<string, string> = {
		'Content-Type': 'application/json',
		...(options.headers as Record<string, string>)
	};

	const apiKey = getApiKey();
	if (apiKey) {
		headers['Authorization'] = `Bearer ${apiKey}`;
	}

	const res = await fetch(url, {
		...options,
		headers
	});

	if (!res.ok) {
		const body = await res.text();
		throw new ApiError(res.status, body);
	}

	return res.json();
}

export class ApiError extends Error {
	constructor(
		public status: number,
		public body: string
	) {
		super(`API error ${status}: ${body}`);
	}
}

// Sessions

export async function listSessions(params?: {
	tool?: string;
	search?: string;
	page?: number;
	per_page?: number;
	sort?: string;
	time_range?: string;
	team_id?: string;
}): Promise<SessionListResponse> {
	const query = new URLSearchParams();
	if (params?.tool) query.set('tool', params.tool);
	if (params?.search) query.set('search', params.search);
	if (params?.page) query.set('page', String(params.page));
	if (params?.per_page) query.set('per_page', String(params.per_page));
	if (params?.sort) query.set('sort', params.sort);
	if (params?.time_range) query.set('time_range', params.time_range);
	if (params?.team_id) query.set('team_id', params.team_id);
	const qs = query.toString();
	return request<SessionListResponse>(`/api/sessions${qs ? `?${qs}` : ''}`);
}

export async function getSession(id: string): Promise<Session> {
	const url = `${getBaseUrl()}/api/sessions/${encodeURIComponent(id)}/raw`;
	const headers: Record<string, string> = {};
	const apiKey = getApiKey();
	if (apiKey) headers['Authorization'] = `Bearer ${apiKey}`;

	const res = await fetch(url, { headers });
	if (!res.ok) {
		const body = await res.text();
		throw new ApiError(res.status, body);
	}

	return parseHailJsonl(await res.text());
}

/** Parse HAIL JSONL text into a Session object */
function parseHailJsonl(text: string): Session {
	const lines = text.split('\n').filter((l) => l.trim().length > 0);
	if (lines.length === 0) throw new Error('Empty JSONL');

	const header = JSON.parse(lines[0]);
	if (header.type !== 'header') throw new Error('First line must be header');

	const events: Session['events'] = [];
	let stats: Session['stats'] | null = null;

	for (let i = 1; i < lines.length; i++) {
		const line = JSON.parse(lines[i]);
		if (line.type === 'event') {
			// Remove the wrapping "type":"event" tag, keep the event fields
			const { type: _, ...event } = line;
			events.push(event);
		} else if (line.type === 'stats') {
			const { type: _, ...s } = line;
			stats = s;
		}
	}

	return {
		version: header.version,
		session_id: header.session_id,
		agent: header.agent,
		context: header.context,
		events,
		stats: stats ?? {
			event_count: events.length,
			message_count: 0,
			tool_call_count: 0,
			task_count: 0,
			duration_seconds: 0,
		},
	};
}

export async function uploadSession(session: Session, teamId: string): Promise<{ id: string; url: string }> {
	return request('/api/sessions', {
		method: 'POST',
		body: JSON.stringify({ session, team_id: teamId })
	});
}

// Teams

export async function listTeams(): Promise<ListTeamsResponse> {
	return request('/api/teams');
}

export async function getTeam(id: string): Promise<TeamDetailResponse> {
	return request<TeamDetailResponse>(`/api/teams/${encodeURIComponent(id)}`);
}

export async function createTeam(data: {
	name: string;
	description?: string;
}): Promise<TeamResponse> {
	return request('/api/teams', {
		method: 'POST',
		body: JSON.stringify(data)
	});
}

export async function updateTeam(id: string, data: {
	name?: string;
	description?: string;
}): Promise<TeamResponse> {
	return request(`/api/teams/${encodeURIComponent(id)}`, {
		method: 'PUT',
		body: JSON.stringify(data)
	});
}

// Team Members

export async function listMembers(teamId: string): Promise<ListMembersResponse> {
	return request(`/api/teams/${encodeURIComponent(teamId)}/members`);
}

export async function addMember(teamId: string, nickname: string): Promise<MemberResponse> {
	return request(`/api/teams/${encodeURIComponent(teamId)}/members`, {
		method: 'POST',
		body: JSON.stringify({ nickname })
	});
}

export async function removeMember(teamId: string, userId: string): Promise<void> {
	await request(`/api/teams/${encodeURIComponent(teamId)}/members/${encodeURIComponent(userId)}`, {
		method: 'DELETE'
	});
}

// Auth / Settings

export async function register(nickname: string): Promise<UserSettings> {
	return request('/api/register', {
		method: 'POST',
		body: JSON.stringify({ nickname })
	});
}

export async function getSettings(): Promise<UserSettings> {
	return request<UserSettings>('/api/auth/me');
}

export async function regenerateApiKey(): Promise<{ api_key: string }> {
	return request('/api/auth/regenerate-key', {
		method: 'POST'
	});
}
