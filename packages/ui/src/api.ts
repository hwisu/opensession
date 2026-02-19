import type { Session, SessionListResponse } from './types';
import { parseHailJsonl } from './hail-parse';

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

export function setApiKey(key: string) {
	localStorage.setItem('opensession_api_key', key);
}

export function clearApiKey() {
	localStorage.removeItem('opensession_api_key');
}

export function setBaseUrl(url: string) {
	localStorage.setItem('opensession_api_url', url);
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
		const msg = body.trimStart().startsWith('<') ? `Server returned ${status}` : body.slice(0, 200);
		super(msg);
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

export async function uploadSession(
	session: Session,
	scopeId: string = 'local',
): Promise<{ id: string; url: string }> {
	return request('/api/sessions', {
		method: 'POST',
		body: JSON.stringify({ session, team_id: scopeId }),
	});
}
