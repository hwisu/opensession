import type {
	Session,
	SessionListResponse,
	GroupResponse,
	GroupDetailResponse,
	ListGroupsResponse,
	UserSettings,
	InviteInfo
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
}): Promise<SessionListResponse> {
	const query = new URLSearchParams();
	if (params?.tool) query.set('tool', params.tool);
	if (params?.search) query.set('search', params.search);
	if (params?.page) query.set('page', String(params.page));
	if (params?.per_page) query.set('per_page', String(params.per_page));
	if (params?.sort) query.set('sort', params.sort);
	if (params?.time_range) query.set('time_range', params.time_range);
	const qs = query.toString();
	return request<SessionListResponse>(`/api/sessions${qs ? `?${qs}` : ''}`);
}

export async function getSession(id: string): Promise<Session> {
	return request<Session>(`/api/sessions/${encodeURIComponent(id)}/raw`);
}

export async function uploadSession(session: Session): Promise<{ session_id: string }> {
	return request('/api/sessions', {
		method: 'POST',
		body: JSON.stringify(session)
	});
}

// Groups

export async function listGroups(): Promise<ListGroupsResponse> {
	return request('/api/groups');
}

export async function getGroup(id: string): Promise<GroupDetailResponse> {
	return request<GroupDetailResponse>(`/api/groups/${encodeURIComponent(id)}`);
}

export async function createGroup(data: {
	name: string;
	description?: string;
	is_public: boolean;
}): Promise<GroupResponse> {
	return request('/api/groups', {
		method: 'POST',
		body: JSON.stringify(data)
	});
}

// Invites

export async function getInviteInfo(code: string): Promise<InviteInfo> {
	return request<InviteInfo>(`/api/invite/${encodeURIComponent(code)}`);
}

export async function joinInvite(code: string): Promise<{ group_id: string }> {
	return request(`/api/invite/${encodeURIComponent(code)}/join`, {
		method: 'POST'
	});
}

// Auth / Settings

export async function register(nickname: string): Promise<UserSettings> {
	return request('/api/auth/register', {
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
