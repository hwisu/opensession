import type {
	AcceptInvitationResponse,
	AuthProvidersResponse,
	AuthTokenResponse,
	InvitationResponse,
	JoinTeamWithKeyResponse,
	ListInvitationsResponse,
	ListMembersResponse,
	ListTeamInviteKeysResponse,
	ListTeamsResponse,
	MemberResponse,
	Session,
	SessionListResponse,
	TeamDetailResponse,
	TeamResponse,
	TeamStatsResponse,
	UserSettings,
} from './types';

// ── Token storage ───────────────────────────────────────────────────────────

function getBaseUrl(): string {
	if (typeof window !== 'undefined') {
		const stored = localStorage.getItem('opensession_api_url');
		if (stored) return stored;
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

function getAccessToken(): string | null {
	if (typeof window !== 'undefined') {
		return localStorage.getItem('opensession_access_token');
	}
	return null;
}

function getRefreshToken(): string | null {
	if (typeof window !== 'undefined') {
		return localStorage.getItem('opensession_refresh_token');
	}
	return null;
}

function getTokenExpiry(): number {
	if (typeof window !== 'undefined') {
		const v = localStorage.getItem('opensession_token_expiry');
		return v ? parseInt(v, 10) : 0;
	}
	return 0;
}

function storeTokens(tokens: AuthTokenResponse) {
	localStorage.setItem('opensession_access_token', tokens.access_token);
	localStorage.setItem('opensession_refresh_token', tokens.refresh_token);
	localStorage.setItem(
		'opensession_token_expiry',
		String(Math.floor(Date.now() / 1000) + tokens.expires_in - 60), // refresh 1 min early
	);
}

function clearTokens() {
	localStorage.removeItem('opensession_access_token');
	localStorage.removeItem('opensession_refresh_token');
	localStorage.removeItem('opensession_token_expiry');
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

/** Check if the user is authenticated (has JWT or API key). */
export function isAuthenticated(): boolean {
	return !!(getAccessToken() || getApiKey());
}

/** Verify auth by refreshing expired tokens. Clears tokens if unrecoverable. */
export async function verifyAuth(): Promise<boolean> {
	if (getApiKey()) return true;

	const token = getAccessToken();
	if (!token) return false;

	const expiry = getTokenExpiry();
	if (expiry > 0 && Math.floor(Date.now() / 1000) >= expiry) {
		const refreshed = await tryRefreshToken();
		if (!refreshed) {
			clearTokens();
			return false;
		}
	}
	return true;
}

// ── Request layer ───────────────────────────────────────────────────────────

/** Get the current auth header value, auto-refreshing JWT if needed. */
async function getAuthHeader(): Promise<string | null> {
	// Prefer JWT
	let token = getAccessToken();
	if (token) {
		// Check if token needs refresh
		const expiry = getTokenExpiry();
		if (expiry > 0 && Math.floor(Date.now() / 1000) >= expiry) {
			const refreshed = await tryRefreshToken();
			if (refreshed) {
				token = getAccessToken();
			} else {
				// Refresh failed, clear tokens
				clearTokens();
				token = null;
			}
		}
		if (token) return `Bearer ${token}`;
	}

	// Fall back to API key
	const apiKey = getApiKey();
	if (apiKey) return `Bearer ${apiKey}`;

	return null;
}

async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
	const url = `${getBaseUrl()}${path}`;
	const headers: Record<string, string> = {
		'Content-Type': 'application/json',
		...(options.headers as Record<string, string>),
	};

	const auth = await getAuthHeader();
	if (auth) {
		headers.Authorization = auth;
	}

	const res = await fetch(url, {
		...options,
		headers,
	});

	if (!res.ok) {
		const body = await res.text();
		throw new ApiError(res.status, body);
	}

	if (res.status === 204) {
		return undefined as T;
	}

	const contentType = res.headers.get('content-type') || '';
	if (!contentType.includes('application/json')) {
		return undefined as T;
	}

	const text = await res.text();
	if (!text.trim()) {
		return undefined as T;
	}

	return JSON.parse(text) as T;
}

export class ApiError extends Error {
	constructor(
		public status: number,
		public body: string,
	) {
		const msg = body.trimStart().startsWith('<') ? `Server returned ${status}` : body.slice(0, 200);
		super(msg);
	}
}

// Sessions

function buildQuery(params?: Record<string, string | number | undefined>): string {
	if (!params) return '';
	const query = new URLSearchParams();
	for (const [key, val] of Object.entries(params)) {
		if (val != null) query.set(key, String(val));
	}
	const qs = query.toString();
	return qs ? `?${qs}` : '';
}

export async function listSessions(params?: {
	tool?: string;
	search?: string;
	page?: number;
	per_page?: number;
	sort?: string;
	time_range?: string;
	team_id?: string;
}): Promise<SessionListResponse> {
	return request<SessionListResponse>(`/api/sessions${buildQuery(params)}`);
}

export async function getSession(id: string): Promise<Session> {
	return requestRaw(`/api/sessions/${encodeURIComponent(id)}/raw`).then(parseHailJsonl);
}

async function requestRaw(path: string): Promise<string> {
	const url = `${getBaseUrl()}${path}`;
	const headers: Record<string, string> = {};
	const auth = await getAuthHeader();
	if (auth) headers.Authorization = auth;

	const res = await fetch(url, { headers });
	if (!res.ok) {
		const body = await res.text();
		throw new ApiError(res.status, body);
	}

	return res.text();
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
			total_input_tokens: 0,
			total_output_tokens: 0,
		},
	};
}

export async function uploadSession(
	session: Session,
	teamId: string,
): Promise<{ id: string; url: string }> {
	return request('/api/sessions', {
		method: 'POST',
		body: JSON.stringify({ session, team_id: teamId }),
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
		body: JSON.stringify(data),
	});
}

export async function updateTeam(
	id: string,
	data: {
		name?: string;
		description?: string;
	},
): Promise<TeamResponse> {
	return request(`/api/teams/${encodeURIComponent(id)}`, {
		method: 'PUT',
		body: JSON.stringify(data),
	});
}

export async function getTeamStats(id: string, timeRange?: string): Promise<TeamStatsResponse> {
	return request<TeamStatsResponse>(
		`/api/teams/${encodeURIComponent(id)}/stats${buildQuery({ time_range: timeRange })}`,
	);
}

// Team Members

export async function listMembers(teamId: string): Promise<ListMembersResponse> {
	return request(`/api/teams/${encodeURIComponent(teamId)}/members`);
}

export async function addMember(teamId: string, nickname: string): Promise<MemberResponse> {
	return request(`/api/teams/${encodeURIComponent(teamId)}/members`, {
		method: 'POST',
		body: JSON.stringify({ nickname }),
	});
}

export async function removeMember(teamId: string, userId: string): Promise<void> {
	await request(`/api/teams/${encodeURIComponent(teamId)}/members/${encodeURIComponent(userId)}`, {
		method: 'DELETE',
	});
}

// Team Invite Keys

export async function listTeamInviteKeys(teamId: string): Promise<ListTeamInviteKeysResponse> {
	return request(`/api/teams/${encodeURIComponent(teamId)}/keys`);
}

export async function createTeamInviteKey(
	teamId: string,
	data?: { role?: 'admin' | 'member'; expires_in_days?: number },
): Promise<{ key_id: string; invite_key: string; role: 'admin' | 'member'; expires_at: string }> {
	return request(`/api/teams/${encodeURIComponent(teamId)}/keys`, {
		method: 'POST',
		body: JSON.stringify(data ?? {}),
	});
}

export async function revokeTeamInviteKey(teamId: string, keyId: string): Promise<void> {
	await request(`/api/teams/${encodeURIComponent(teamId)}/keys/${encodeURIComponent(keyId)}`, {
		method: 'DELETE',
	});
}

export async function listTeamInvitations(teamId: string): Promise<ListInvitationsResponse> {
	return request(`/api/teams/${encodeURIComponent(teamId)}/invitations`);
}

export async function cancelTeamInvitation(teamId: string, invitationId: string): Promise<void> {
	await request(`/api/teams/${encodeURIComponent(teamId)}/invitations/${encodeURIComponent(invitationId)}`, {
		method: 'DELETE',
	});
}

export async function joinTeamWithKey(inviteKey: string): Promise<JoinTeamWithKeyResponse> {
	return request('/api/teams/join-with-key', {
		method: 'POST',
		body: JSON.stringify({ invite_key: inviteKey }),
	});
}

// ── Auth (legacy, CLI-compatible) ───────────────────────────────────────────

export async function register(nickname: string): Promise<UserSettings> {
	return request('/api/register', {
		method: 'POST',
		body: JSON.stringify({ nickname }),
	});
}

export async function getSettings(): Promise<UserSettings> {
	return request<UserSettings>('/api/auth/me');
}

export async function regenerateApiKey(): Promise<{ api_key: string }> {
	return request('/api/auth/regenerate-key', {
		method: 'POST',
	});
}

// ── Auth (email/password + JWT) ─────────────────────────────────────────────

export async function authRegister(
	email: string,
	password: string,
	nickname: string,
): Promise<AuthTokenResponse> {
	const url = `${getBaseUrl()}/api/auth/register`;
	const res = await fetch(url, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ email, password, nickname }),
	});
	if (!res.ok) {
		const body = await res.text();
		throw new ApiError(res.status, body);
	}
	const tokens: AuthTokenResponse = await res.json();
	storeTokens(tokens);
	return tokens;
}

export async function authLogin(email: string, password: string): Promise<AuthTokenResponse> {
	const url = `${getBaseUrl()}/api/auth/login`;
	const res = await fetch(url, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ email, password }),
	});
	if (!res.ok) {
		const body = await res.text();
		throw new ApiError(res.status, body);
	}
	const tokens: AuthTokenResponse = await res.json();
	storeTokens(tokens);
	return tokens;
}

async function tryRefreshToken(): Promise<boolean> {
	const refreshToken = getRefreshToken();
	if (!refreshToken) return false;

	try {
		const url = `${getBaseUrl()}/api/auth/refresh`;
		const res = await fetch(url, {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify({ refresh_token: refreshToken }),
		});
		if (!res.ok) return false;
		const tokens: AuthTokenResponse = await res.json();
		storeTokens(tokens);
		return true;
	} catch {
		return false;
	}
}

export async function authLogout(): Promise<void> {
	const refreshToken = getRefreshToken();
	if (refreshToken) {
		try {
			await request('/api/auth/logout', {
				method: 'POST',
				body: JSON.stringify({ refresh_token: refreshToken }),
			});
		} catch {
			// Ignore errors during logout
		}
	}
	clearTokens();
	clearApiKey();
}

export async function changePassword(currentPassword: string, newPassword: string): Promise<void> {
	await request('/api/auth/password', {
		method: 'PUT',
		body: JSON.stringify({ current_password: currentPassword, new_password: newPassword }),
	});
}

/** Get available auth providers from the server. */
export async function getAuthProviders(): Promise<AuthProvidersResponse> {
	const url = `${getBaseUrl()}/api/auth/providers`;
	const res = await fetch(url);
	if (!res.ok) return { email_password: true, oauth: [] };
	return res.json();
}

/** Get the OAuth redirect URL for a given provider. */
export function getOAuthUrl(provider: string): string {
	return `${getBaseUrl()}/api/auth/oauth/${encodeURIComponent(provider)}`;
}

/** Initiate OAuth linking for an authenticated user (from Settings). */
export async function linkOAuth(provider: string): Promise<void> {
	const resp = await request<{ url: string }>(
		`/api/auth/oauth/${encodeURIComponent(provider)}/link`,
		{ method: 'POST' },
	);
	window.location.href = resp.url;
}

// Legacy aliases for backward compatibility
export function getGitHubAuthUrl(): string {
	return getOAuthUrl('github');
}

export async function linkGitHub(): Promise<void> {
	return linkOAuth('github');
}

// ── Invitations ─────────────────────────────────────────────────────────────

export async function inviteMember(
	teamId: string,
	data: {
		email?: string;
		oauth_provider?: string;
		oauth_provider_username?: string;
		role?: string;
	},
): Promise<InvitationResponse> {
	return request(`/api/teams/${encodeURIComponent(teamId)}/invite`, {
		method: 'POST',
		body: JSON.stringify(data),
	});
}

export async function listInvitations(): Promise<ListInvitationsResponse> {
	return request('/api/invitations');
}

export async function acceptInvitation(id: string): Promise<AcceptInvitationResponse> {
	return request(`/api/invitations/${encodeURIComponent(id)}/accept`, {
		method: 'POST',
	});
}

export async function declineInvitation(id: string): Promise<void> {
	await request(`/api/invitations/${encodeURIComponent(id)}/decline`, {
		method: 'POST',
	});
}

/** Handle OAuth callback: parse tokens from URL fragment. */
export function handleAuthCallback(): AuthTokenResponse | null {
	if (typeof window === 'undefined') return null;

	const hash = window.location.hash.slice(1);
	if (!hash) return null;

	const params = new URLSearchParams(hash);
	const accessToken = params.get('access_token');
	const refreshToken = params.get('refresh_token');
	const expiresIn = params.get('expires_in');

	if (!accessToken || !refreshToken || !expiresIn) return null;

	const tokens: AuthTokenResponse = {
		access_token: accessToken,
		refresh_token: refreshToken,
		expires_in: parseInt(expiresIn, 10),
		user_id: '',
		nickname: '',
	};

	storeTokens(tokens);
	// Clean up URL fragment
	window.history.replaceState(null, '', window.location.pathname);
	return tokens;
}
