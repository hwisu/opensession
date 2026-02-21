import { parseHailJsonl } from './hail-parse';
import type {
	AuthProvidersResponse,
	AuthTokenResponse,
	CapabilitiesResponse,
	ParsePreviewErrorResponse,
	ParsePreviewRequest,
	ParsePreviewResponse,
	ParseSource,
	Session,
	SessionListResponse,
	UserSettings,
} from './types';

function getBaseUrl(): string {
	if (typeof window !== 'undefined') {
		const stored = localStorage.getItem('opensession_api_url');
		if (stored) return stored;
		return window.location.origin;
	}
	return '';
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
		if (!v) return 0;
		const parsed = parseInt(v, 10);
		if (!Number.isFinite(parsed) || parsed <= 0) return 0;
		// Legacy clients stored token expiry as epoch milliseconds.
		const normalized = parsed > 100_000_000_000 ? Math.floor(parsed / 1000) : parsed;
		if (normalized !== parsed) {
			localStorage.setItem('opensession_token_expiry', String(normalized));
		}
		return normalized;
	}
	return 0;
}

function storeTokens(tokens: AuthTokenResponse) {
	localStorage.setItem('opensession_access_token', tokens.access_token);
	localStorage.setItem('opensession_refresh_token', tokens.refresh_token);
	localStorage.setItem(
		'opensession_token_expiry',
		String(Math.floor(Date.now() / 1000) + tokens.expires_in - 60),
	);
}

function clearTokens() {
	localStorage.removeItem('opensession_access_token');
	localStorage.removeItem('opensession_refresh_token');
	localStorage.removeItem('opensession_token_expiry');
}

export function setBaseUrl(url: string) {
	localStorage.setItem('opensession_api_url', url);
}

export function isAuthenticated(): boolean {
	// UI login state is token-session based; API keys are treated as request credentials only.
	return !!getAccessToken();
}

export async function verifyAuth(): Promise<boolean> {
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

async function getAuthHeader(): Promise<string | null> {
	let token = getAccessToken();
	if (token) {
		const expiry = getTokenExpiry();
		if (expiry > 0 && Math.floor(Date.now() / 1000) >= expiry) {
			const refreshed = await tryRefreshToken();
			if (refreshed) {
				token = getAccessToken();
			} else {
				clearTokens();
				token = null;
			}
		}
		if (token) return `Bearer ${token}`;
	}
	return null;
}

export class ApiError extends Error {
	constructor(
		public status: number,
		public body: string,
	) {
		let msg = body.trimStart().startsWith('<') ? `Server returned ${status}` : body.slice(0, 200);
		if (!body.trimStart().startsWith('<')) {
			try {
				const parsed = JSON.parse(body) as { message?: unknown };
				if (typeof parsed.message === 'string' && parsed.message.trim()) {
					msg = parsed.message.trim();
				}
			} catch {
				// ignore non-json error bodies
			}
		}
		super(msg);
	}
}

export class PreviewApiError extends Error {
	constructor(
		public status: number,
		public payload: ParsePreviewErrorResponse,
	) {
		super(payload.message);
	}
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

export async function uploadSession(session: Session): Promise<{ id: string; url: string }> {
	return request('/api/sessions', {
		method: 'POST',
		body: JSON.stringify({ session }),
	});
}

export async function getSettings(): Promise<UserSettings> {
	return request<UserSettings>('/api/auth/me');
}

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
			// ignore errors on logout
		}
	}
	clearTokens();
}

export async function getAuthProviders(): Promise<AuthProvidersResponse> {
	const url = `${getBaseUrl()}/api/auth/providers`;
	const res = await fetch(url);
	if (!res.ok) return { email_password: false, oauth: [] };
	return res.json();
}

export async function getApiCapabilities(): Promise<CapabilitiesResponse> {
	const url = `${getBaseUrl()}/api/capabilities`;
	try {
		const res = await fetch(url);
		if (res.ok) {
			return res.json();
		}
	} catch {
		// ignore and fall through
	}
	return {
		auth_enabled: false,
		upload_enabled: false,
		ingest_preview_enabled: false,
		gh_share_enabled: false,
	};
}

export async function isAuthApiAvailable(): Promise<boolean> {
	const capabilities = await getApiCapabilities();
	return capabilities.auth_enabled;
}

export async function isUploadApiAvailable(): Promise<boolean> {
	const capabilities = await getApiCapabilities();
	return capabilities.upload_enabled;
}

export async function isIngestPreviewApiAvailable(): Promise<boolean> {
	const capabilities = await getApiCapabilities();
	return capabilities.ingest_preview_enabled;
}

export async function isGhShareAvailable(): Promise<boolean> {
	const capabilities = await getApiCapabilities();
	return capabilities.gh_share_enabled;
}

async function postIngestPreview(req: ParsePreviewRequest): Promise<ParsePreviewResponse> {
	const url = `${getBaseUrl()}/api/ingest/preview`;
	const headers: Record<string, string> = { 'Content-Type': 'application/json' };
	const auth = await getAuthHeader();
	if (auth) headers.Authorization = auth;

	const res = await fetch(url, {
		method: 'POST',
		headers,
		body: JSON.stringify(req),
	});

	const body = await res.text();
	if (!res.ok) {
		let parsed: ParsePreviewErrorResponse | null = null;
		try {
			parsed = JSON.parse(body) as ParsePreviewErrorResponse;
		} catch {
			parsed = null;
		}
		if (parsed && typeof parsed.code === 'string' && typeof parsed.message === 'string') {
			throw new PreviewApiError(res.status, parsed);
		}
		throw new ApiError(res.status, body);
	}

	if (!body.trim()) {
		throw new ApiError(res.status, 'Empty ingest preview response');
	}
	return JSON.parse(body) as ParsePreviewResponse;
}

export async function previewSessionFromGithubSource(params: {
	owner: string;
	repo: string;
	ref: string;
	path: string;
	parser_hint?: string;
}): Promise<ParsePreviewResponse> {
	const source: ParseSource = {
		kind: 'github',
		owner: params.owner,
		repo: params.repo,
		ref: params.ref,
		path: params.path,
	};
	return postIngestPreview({
		source,
		parser_hint: params.parser_hint ?? null,
	});
}

export async function previewSessionFromGitSource(params: {
	remote: string;
	ref: string;
	path: string;
	parser_hint?: string;
}): Promise<ParsePreviewResponse> {
	const source: ParseSource = {
		kind: 'git',
		remote: params.remote,
		ref: params.ref,
		path: params.path,
	};
	return postIngestPreview({
		source,
		parser_hint: params.parser_hint ?? null,
	});
}

export async function previewSessionFromInlineSource(params: {
	filename: string;
	content_base64: string;
	parser_hint?: string;
}): Promise<ParsePreviewResponse> {
	const source: ParseSource = {
		kind: 'inline',
		filename: params.filename,
		content_base64: params.content_base64,
	};
	return postIngestPreview({
		source,
		parser_hint: params.parser_hint ?? null,
	});
}

export function getParsePreviewError(error: unknown): ParsePreviewErrorResponse | null {
	if (error instanceof PreviewApiError) return error.payload;
	if (error instanceof ApiError) {
		try {
			const parsed = JSON.parse(error.body) as ParsePreviewErrorResponse;
			if (typeof parsed.code === 'string' && typeof parsed.message === 'string') {
				return parsed;
			}
		} catch {
			// ignore non-json errors
		}
	}
	return null;
}

export function getOAuthUrl(provider: string): string {
	return `${getBaseUrl()}/api/auth/oauth/${encodeURIComponent(provider)}`;
}

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
	window.history.replaceState(null, '', window.location.pathname);
	return tokens;
}
