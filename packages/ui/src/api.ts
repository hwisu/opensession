import { parseHailJsonl } from './hail-parse';
import type {
	AuthProvidersResponse,
	AuthTokenResponse,
	CapabilitiesResponse,
	ParsePreviewErrorResponse,
	ParsePreviewRequest,
	ParsePreviewResponse,
	ParseSource,
	IssueApiKeyResponse,
	GitCredentialSummary,
	ListGitCredentialsResponse,
	LocalReviewBundle,
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

function getApiKey(): string | null {
	if (typeof window === 'undefined') return null;
	return localStorage.getItem('opensession_api_key');
}

function getCookie(name: string): string | null {
	if (typeof window === 'undefined') return null;
	const encodedName = `${name}=`;
	const parts = document.cookie.split(';');
	for (const raw of parts) {
		const trimmed = raw.trim();
		if (trimmed.startsWith(encodedName)) {
			return trimmed.slice(encodedName.length);
		}
	}
	return null;
}

function getCsrfToken(): string | null {
	return getCookie('opensession_csrf_token');
}

export function setBaseUrl(url: string) {
	localStorage.setItem('opensession_api_url', url);
}

export function isAuthenticated(): boolean {
	if (typeof window === 'undefined') return false;
	return getCsrfToken() != null;
}

export async function verifyAuth(): Promise<boolean> {
	try {
		await request<unknown>('/api/auth/verify', { method: 'POST' });
		return true;
	} catch (error) {
		if (error instanceof ApiError && (error.status === 401 || error.status === 403)) {
			const refreshed = await tryRefreshToken();
			if (!refreshed) return false;
			try {
				await request<unknown>('/api/auth/verify', { method: 'POST' });
				return true;
			} catch {
				return false;
			}
		}
		return false;
	}
}

async function getAuthHeader(): Promise<string | null> {
	const apiKey = getApiKey();
	if (apiKey) return `Bearer ${apiKey}`;
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
	const method = (options.method ?? 'GET').toUpperCase();
	const needsCsrf = method !== 'GET' && method !== 'HEAD' && method !== 'OPTIONS';
	const headers: Record<string, string> = {
		'Content-Type': 'application/json',
		...(options.headers as Record<string, string>),
	};

	const auth = await getAuthHeader();
	if (auth) {
		headers.Authorization = auth;
	}
	if (needsCsrf) {
		const csrf = getCsrfToken();
		if (csrf) headers['X-CSRF-Token'] = csrf;
	}

	const res = await fetch(url, {
		...options,
		headers,
		credentials: 'include',
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

export async function getLocalReviewBundle(reviewId: string): Promise<LocalReviewBundle> {
	return request<LocalReviewBundle>(`/api/review/local/${encodeURIComponent(reviewId)}`);
}

async function requestRaw(path: string): Promise<string> {
	const url = `${getBaseUrl()}${path}`;
	const headers: Record<string, string> = {};
	const auth = await getAuthHeader();
	if (auth) headers.Authorization = auth;

	const res = await fetch(url, { headers, credentials: 'include' });
	if (!res.ok) {
		const body = await res.text();
		throw new ApiError(res.status, body);
	}

	return res.text();
}

export async function getSettings(): Promise<UserSettings> {
	return request<UserSettings>('/api/auth/me');
}

export async function issueApiKey(): Promise<IssueApiKeyResponse> {
	return request<IssueApiKeyResponse>('/api/auth/api-keys/issue', {
		method: 'POST',
	});
}

export async function listGitCredentials(): Promise<GitCredentialSummary[]> {
	const response = await request<ListGitCredentialsResponse>('/api/auth/git-credentials');
	return response.credentials ?? [];
}

export async function createGitCredential(params: {
	label: string;
	host: string;
	path_prefix?: string | null;
	header_name: string;
	header_value: string;
}): Promise<GitCredentialSummary> {
	return request<GitCredentialSummary>('/api/auth/git-credentials', {
		method: 'POST',
		body: JSON.stringify({
			label: params.label,
			host: params.host,
			path_prefix: params.path_prefix ?? null,
			header_name: params.header_name,
			header_value: params.header_value,
		}),
	});
}

export async function deleteGitCredential(id: string): Promise<void> {
	await request('/api/auth/git-credentials/' + encodeURIComponent(id), {
		method: 'DELETE',
	});
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
		credentials: 'include',
	});
	if (!res.ok) {
		const body = await res.text();
		throw new ApiError(res.status, body);
	}
	return (await res.json()) as AuthTokenResponse;
}

export async function authLogin(email: string, password: string): Promise<AuthTokenResponse> {
	const url = `${getBaseUrl()}/api/auth/login`;
	const res = await fetch(url, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ email, password }),
		credentials: 'include',
	});
	if (!res.ok) {
		const body = await res.text();
		throw new ApiError(res.status, body);
	}
	return (await res.json()) as AuthTokenResponse;
}

async function tryRefreshToken(): Promise<boolean> {
	try {
		const url = `${getBaseUrl()}/api/auth/refresh`;
		const headers: Record<string, string> = { 'Content-Type': 'application/json' };
		const csrf = getCsrfToken();
		if (csrf) headers['X-CSRF-Token'] = csrf;
		const res = await fetch(url, {
			method: 'POST',
			headers,
			credentials: 'include',
		});
		if (!res.ok) return false;
		await res.json();
		return true;
	} catch {
		return false;
	}
}

export async function authLogout(): Promise<void> {
	try {
		await request('/api/auth/logout', {
			method: 'POST',
		});
	} catch {
		// ignore errors on logout
	}
}

export async function getAuthProviders(): Promise<AuthProvidersResponse> {
	const url = `${getBaseUrl()}/api/auth/providers`;
	const res = await fetch(url, { credentials: 'include' });
	if (!res.ok) return { email_password: false, oauth: [] };
	return res.json();
}

export async function getApiCapabilities(): Promise<CapabilitiesResponse> {
	const url = `${getBaseUrl()}/api/capabilities`;
	try {
		const res = await fetch(url, { credentials: 'include' });
		if (res.ok) {
			return res.json();
		}
	} catch {
		// ignore and fall through
	}
	return {
		auth_enabled: false,
		parse_preview_enabled: false,
		register_targets: [],
		share_modes: [],
	};
}

export async function isAuthApiAvailable(): Promise<boolean> {
	const capabilities = await getApiCapabilities();
	return capabilities.auth_enabled;
}

export async function isParsePreviewApiAvailable(): Promise<boolean> {
	const capabilities = await getApiCapabilities();
	return capabilities.parse_preview_enabled;
}

async function postParsePreview(req: ParsePreviewRequest): Promise<ParsePreviewResponse> {
	const url = `${getBaseUrl()}/api/parse/preview`;
	const headers: Record<string, string> = { 'Content-Type': 'application/json' };
	const auth = await getAuthHeader();
	if (auth) headers.Authorization = auth;
	const csrf = getCsrfToken();
	if (csrf) headers['X-CSRF-Token'] = csrf;

	const res = await fetch(url, {
		method: 'POST',
		headers,
		body: JSON.stringify(req),
		credentials: 'include',
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
		throw new ApiError(res.status, 'Empty parse preview response');
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
	return postParsePreview({
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
	return postParsePreview({
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
	return postParsePreview({
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

export async function handleAuthCallback(): Promise<boolean> {
	if (typeof window === 'undefined') return false;
	if (window.location.hash) {
		window.history.replaceState(null, '', window.location.pathname);
	}
	try {
		await request<unknown>('/api/auth/verify', { method: 'POST' });
		return true;
	} catch {
		return false;
	}
}
