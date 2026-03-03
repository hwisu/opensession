import type {
	AuthProvidersResponse,
	CapabilitiesResponse,
	DesktopApiError,
	DesktopContractVersionResponse,
	DesktopHandoffBuildRequest,
	DesktopHandoffBuildResponse,
	DesktopSessionListQuery,
	SessionDetail,
	SessionListResponse,
	SessionRepoListResponse,
} from './types';

export type SessionListParams = {
	tool?: string;
	git_repo_name?: string;
	search?: string;
	page?: number;
	per_page?: number;
	sort?: string;
	time_range?: string;
};

export type DesktopInvoke = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

export const DESKTOP_CONTRACT_VERSION = 'desktop-ipc-v1';

type ErrorDetails = Record<string, unknown> | null;

export class SessionAdapterError extends Error {
	constructor(
		public code: string,
		public status: number,
		public body: string,
		public details: ErrorDetails = null,
	) {
		super(body);
	}
}

export interface SessionReadAdapter {
	listSessions(params?: SessionListParams): Promise<SessionListResponse>;
	listRepos(): Promise<string[]>;
	getSessionDetail(id: string): Promise<SessionDetail>;
	getSessionRaw(id: string): Promise<string>;
	buildHandoff(sessionId: string, pinLatest?: boolean): Promise<DesktopHandoffBuildResponse>;
	getCapabilities(): Promise<CapabilitiesResponse>;
	getAuthProviders(): Promise<AuthProvidersResponse>;
	getContractVersion(): Promise<string>;
}

function buildQuery(params?: SessionListParams): string {
	if (!params) return '';
	const query = new URLSearchParams();
	for (const [key, val] of Object.entries(params)) {
		if (val != null) query.set(key, String(val));
	}
	const qs = query.toString();
	return qs ? `?${qs}` : '';
}

function toSessionListQuery(params?: SessionListParams): DesktopSessionListQuery {
	return {
		page: params?.page != null ? String(params.page) : null,
		per_page: params?.per_page != null ? String(params.per_page) : null,
		search: params?.search ?? null,
		tool: params?.tool ?? null,
		git_repo_name: params?.git_repo_name ?? null,
		sort: params?.sort ?? null,
		time_range: params?.time_range ?? null,
	};
}

async function readJson<T>(res: Response): Promise<T> {
	if (res.status === 204) return undefined as T;
	const contentType = res.headers.get('content-type') || '';
	if (!contentType.includes('application/json')) return undefined as T;
	const text = await res.text();
	if (!text.trim()) return undefined as T;
	return JSON.parse(text) as T;
}

function serializeErrorBody(payload: {
	code: string;
	message: string;
	details?: ErrorDetails;
}): string {
	return JSON.stringify({
		code: payload.code,
		message: payload.message,
		details: payload.details ?? null,
	});
}

function normalizeSessionAdapterErrorPayload(
	payload: Partial<DesktopApiError> | null | undefined,
	fallbackCode: string,
	fallbackMessage: string,
): { code: string; message: string; details: ErrorDetails } {
	return {
		code: typeof payload?.code === 'string' && payload.code.trim() ? payload.code : fallbackCode,
		message:
			typeof payload?.message === 'string' && payload.message.trim()
				? payload.message
				: fallbackMessage,
		details:
			payload && typeof payload.details === 'object'
				? (payload.details as Record<string, unknown>)
				: null,
	};
}

function parsePossibleDesktopError(value: unknown): Partial<DesktopApiError> | null {
	if (!value || typeof value !== 'object') return null;
	const candidate = value as Partial<DesktopApiError> & { error?: unknown };
	if (candidate.error && typeof candidate.error === 'object') {
		return parsePossibleDesktopError(candidate.error);
	}
	return candidate;
}

function normalizeDesktopInvokeError(error: unknown): SessionAdapterError {
	const parsed = parsePossibleDesktopError(error);
	const status = typeof parsed?.status === 'number' ? parsed.status : 500;
	const normalized = normalizeSessionAdapterErrorPayload(
		parsed,
		'desktop_bridge_request_failed',
		'Desktop bridge request failed',
	);
	return new SessionAdapterError(
		normalized.code,
		status,
		serializeErrorBody(normalized),
		normalized.details,
	);
}

export function createWebSessionReadAdapter(args: {
	baseUrl: string;
	fetchImpl: typeof fetch;
	getAuthHeader: () => Promise<string | null>;
}): SessionReadAdapter {
	const { baseUrl, fetchImpl, getAuthHeader } = args;

	async function requestJson<T>(path: string): Promise<T> {
		const headers: Record<string, string> = {};
		const auth = await getAuthHeader();
		if (auth) headers.Authorization = auth;
		const res = await fetchImpl(`${baseUrl}${path}`, {
			method: 'GET',
			headers,
			credentials: 'include',
		});
		if (!res.ok) {
			const body = await res.text();
			throw new SessionAdapterError('http_request_failed', res.status, body);
		}
		return readJson<T>(res);
	}

	async function requestRaw(path: string): Promise<string> {
		const headers: Record<string, string> = {};
		const auth = await getAuthHeader();
		if (auth) headers.Authorization = auth;
		const res = await fetchImpl(`${baseUrl}${path}`, {
			method: 'GET',
			headers,
			credentials: 'include',
		});
		if (!res.ok) {
			const body = await res.text();
			throw new SessionAdapterError('http_request_failed', res.status, body);
		}
		return res.text();
	}

	return {
		listSessions(params) {
			return requestJson<SessionListResponse>(`/api/sessions${buildQuery(params)}`);
		},
		async listRepos() {
			const response = await requestJson<SessionRepoListResponse>('/api/sessions/repos');
			return response.repos ?? [];
		},
		getSessionDetail(id) {
			return requestJson<SessionDetail>(`/api/sessions/${encodeURIComponent(id)}`);
		},
		getSessionRaw(id) {
			return requestRaw(`/api/sessions/${encodeURIComponent(id)}/raw`);
		},
		async buildHandoff() {
			throw new SessionAdapterError(
				'desktop_handoff_unsupported',
				501,
				serializeErrorBody({
					code: 'desktop_handoff_unsupported',
					message: 'Handoff build is available only in desktop runtime.',
				}),
			);
		},
		getCapabilities() {
			return requestJson<CapabilitiesResponse>('/api/capabilities');
		},
		getAuthProviders() {
			return requestJson<AuthProvidersResponse>('/api/auth/providers');
		},
		async getContractVersion() {
			return DESKTOP_CONTRACT_VERSION;
		},
	};
}

export function createDesktopSessionReadAdapter(invoke: DesktopInvoke): SessionReadAdapter {
	let contractCheck: Promise<void> | null = null;

	async function invokeWithNormalization<T>(
		cmd: string,
		args?: Record<string, unknown>,
	): Promise<T> {
		try {
			return await invoke<T>(cmd, args);
		} catch (error) {
			throw normalizeDesktopInvokeError(error);
		}
	}

	async function getDesktopContractVersion(): Promise<string> {
		const payload = await invokeWithNormalization<DesktopContractVersionResponse>(
			'desktop_get_contract_version',
		);
		return payload.version;
	}

	async function ensureContractVersion(): Promise<void> {
		if (!contractCheck) {
			contractCheck = (async () => {
				const actual = await getDesktopContractVersion();
				if (actual !== DESKTOP_CONTRACT_VERSION) {
					throw new SessionAdapterError(
						'desktop_contract_mismatch',
						409,
						serializeErrorBody({
							code: 'desktop_contract_mismatch',
							message:
								'Desktop contract mismatch detected. Update desktop runtime and web bundle to the same version.',
							details: {
								expected: DESKTOP_CONTRACT_VERSION,
								actual,
							},
						}),
						{
							expected: DESKTOP_CONTRACT_VERSION,
							actual,
						},
					);
				}
			})().catch((error) => {
				contractCheck = null;
				throw error;
			});
		}
		await contractCheck;
	}

	async function invokeAfterContractCheck<T>(
		cmd: string,
		args?: Record<string, unknown>,
	): Promise<T> {
		await ensureContractVersion();
		return invokeWithNormalization<T>(cmd, args);
	}

	return {
		async listSessions(params) {
			return invokeAfterContractCheck<SessionListResponse>('desktop_list_sessions', {
				query: toSessionListQuery(params),
			});
		},
		async listRepos() {
			const response =
				await invokeAfterContractCheck<SessionRepoListResponse>('desktop_list_repos');
			return response.repos ?? [];
		},
		async getSessionDetail(id) {
			return invokeAfterContractCheck<SessionDetail>('desktop_get_session_detail', { id });
		},
		async getSessionRaw(id) {
			return invokeAfterContractCheck<string>('desktop_get_session_raw', { id });
		},
		async buildHandoff(sessionId, pinLatest = true) {
			const request: DesktopHandoffBuildRequest = {
				session_id: sessionId,
				pin_latest: pinLatest,
			};
			return invokeAfterContractCheck<DesktopHandoffBuildResponse>('desktop_build_handoff', {
				request,
			});
		},
		async getCapabilities() {
			return invokeAfterContractCheck<CapabilitiesResponse>('desktop_get_capabilities');
		},
		async getAuthProviders() {
			return invokeAfterContractCheck<AuthProvidersResponse>('desktop_get_auth_providers');
		},
		async getContractVersion() {
			return getDesktopContractVersion();
		},
	};
}
