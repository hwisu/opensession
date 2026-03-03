import assert from 'node:assert/strict';
import test from 'node:test';
import {
	ApiError,
	buildSessionHandoff,
	getApiCapabilities,
	getAuthProviders,
	getSessionDetail,
	listSessionRepos,
	listSessions,
	setBaseUrl,
} from './api.ts';

type DesktopInvoke = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;

type BrowserEnvOptions = {
	origin: string;
	tauriRuntime?: boolean;
	storedBaseUrl?: string;
	runtimeOverride?: string;
	invoke?: DesktopInvoke;
};

type TestWindow = Window & {
	__TAURI__?: { core?: { invoke?: DesktopInvoke } };
};

type InvokeCall = {
	cmd: string;
	args?: Record<string, unknown>;
};

const globals = globalThis as typeof globalThis & {
	window?: TestWindow;
	localStorage?: Storage;
	fetch?: typeof fetch;
};

const originalWindow = globals.window;
const originalLocalStorage = globals.localStorage;
const originalFetch = globals.fetch;

function restoreGlobals() {
	if (originalWindow === undefined) delete globals.window;
	else globals.window = originalWindow;

	if (originalLocalStorage === undefined) delete globals.localStorage;
	else globals.localStorage = originalLocalStorage;

	if (originalFetch === undefined) delete globals.fetch;
	else globals.fetch = originalFetch;
}

function installBrowserEnv({
	origin,
	tauriRuntime = false,
	storedBaseUrl,
	runtimeOverride,
	invoke,
}: BrowserEnvOptions) {
	const storage = new Map<string, string>();
	if (storedBaseUrl) storage.set('opensession_api_url', storedBaseUrl);

	globals.localStorage = {
		getItem(key: string): string | null {
			return storage.has(key) ? (storage.get(key) ?? null) : null;
		},
		setItem(key: string, value: string) {
			storage.set(key, value);
		},
		removeItem(key: string) {
			storage.delete(key);
		},
		clear() {
			storage.clear();
		},
		key(index: number): string | null {
			return [...storage.keys()][index] ?? null;
		},
		get length(): number {
			return storage.size;
		},
	} as Storage;

	const protocol = origin.startsWith('tauri://') ? 'tauri:' : new URL(origin).protocol;
	const win = {
		location: { origin, protocol },
	} as unknown as TestWindow;

	if (tauriRuntime) {
		win.__TAURI_INTERNALS__ = {};
	}
	if (invoke) {
		win.__TAURI__ = { core: { invoke } };
	}
	if (runtimeOverride) {
		win.__OPENSESSION_API_URL__ = runtimeOverride;
	}

	globals.window = win;
}

function installFetchProbe(calls: string[], payload: unknown = { total: 0, sessions: [] }) {
	globals.fetch = (async (input: RequestInfo | URL): Promise<Response> => {
		calls.push(String(input));
		return new Response(JSON.stringify(payload), {
			status: 200,
			headers: { 'content-type': 'application/json' },
		});
	}) as typeof fetch;
}

function installInvokeProbe(calls: InvokeCall[]) {
	return async (cmd: string, args?: Record<string, unknown>): Promise<unknown> => {
		calls.push({ cmd, args });
			switch (cmd) {
				case 'desktop_get_contract_version':
					return { version: 'desktop-ipc-v1' };
				case 'desktop_list_sessions':
					return { total: 3, page: 2, per_page: 30, sessions: [] };
				case 'desktop_list_repos':
					return { repos: ['acme/api', 'acme/web'] };
			case 'desktop_get_session_detail':
					return {
						id: args?.id ?? 'unknown',
						user_id: null,
						nickname: null,
						tool: 'codex',
						agent_provider: null,
						agent_model: null,
						title: null,
						description: null,
						tags: null,
						created_at: '2026-03-03T00:00:00Z',
						uploaded_at: '2026-03-03T00:00:00Z',
						message_count: 0,
						task_count: 0,
						event_count: 0,
						duration_seconds: 0,
						total_input_tokens: 0,
						total_output_tokens: 0,
						has_errors: false,
						max_active_agents: 0,
						session_score: 0,
						score_plugin: 'default',
						linked_sessions: [],
					};
				case 'desktop_build_handoff':
					return {
						artifact_uri: 'os://artifact/test123',
						pinned_alias: 'latest',
					};
			case 'desktop_get_capabilities':
				return {
					auth_enabled: false,
					parse_preview_enabled: false,
					register_targets: [],
					share_modes: [],
				};
			case 'desktop_get_auth_providers':
				return { email_password: false, oauth: [] };
			default:
				throw new Error(`unexpected command: ${cmd}`);
		}
	};
}

test.afterEach(() => {
	restoreGlobals();
});

test('listSessions uses current web origin by default', async () => {
	installBrowserEnv({ origin: 'http://127.0.0.1:5173' });
	const calls: string[] = [];
	installFetchProbe(calls);

	await listSessions();

	assert.equal(calls[0], 'http://127.0.0.1:5173/api/sessions');
});

test('listSessions prefers stored base URL over origin', async () => {
	installBrowserEnv({
		origin: 'http://127.0.0.1:5173',
		storedBaseUrl: 'http://localhost:3900',
	});
	const calls: string[] = [];
	installFetchProbe(calls);

	await listSessions();

	assert.equal(calls[0], 'http://localhost:3900/api/sessions');
});

test('listSessions falls back to local API URL when invoke bridge is unavailable', async () => {
	installBrowserEnv({ origin: 'tauri://localhost', tauriRuntime: true });
	const calls: string[] = [];
	installFetchProbe(calls);

	await listSessions();

	assert.equal(calls[0], 'http://127.0.0.1:3000/api/sessions');
});

test('desktop runtime uses invoke bridge for listSessions when available', async () => {
	const invokeCalls: InvokeCall[] = [];
	installBrowserEnv({
		origin: 'tauri://localhost',
		tauriRuntime: true,
		invoke: installInvokeProbe(invokeCalls),
	});
	const fetchCalls: string[] = [];
	installFetchProbe(fetchCalls);

	const response = await listSessions({
		page: 2,
		per_page: 30,
		search: 'fix',
		tool: 'codex',
		git_repo_name: 'org/repo',
	});

	assert.equal(fetchCalls.length, 0);
	assert.equal(invokeCalls[0]?.cmd, 'desktop_get_contract_version');
	assert.equal(invokeCalls[1]?.cmd, 'desktop_list_sessions');
	assert.deepEqual((invokeCalls[1]?.args as { query?: unknown })?.query, {
		page: '2',
		per_page: '30',
		search: 'fix',
		tool: 'codex',
		git_repo_name: 'org/repo',
		sort: null,
		time_range: null,
	});
	assert.equal(response.total, 3);
});

test('desktop bridge decodes encoded session id for session detail', async () => {
	const invokeCalls: InvokeCall[] = [];
	installBrowserEnv({
		origin: 'tauri://localhost',
		tauriRuntime: true,
		invoke: installInvokeProbe(invokeCalls),
	});
	const fetchCalls: string[] = [];
	installFetchProbe(fetchCalls);

	const detail = await getSessionDetail('team/alpha');

	assert.equal(fetchCalls.length, 0);
	assert.equal(invokeCalls[0]?.cmd, 'desktop_get_contract_version');
	assert.equal(invokeCalls[1]?.cmd, 'desktop_get_session_detail');
	assert.deepEqual(invokeCalls[1]?.args, { id: 'team/alpha' });
	assert.equal(detail.id, 'team/alpha');
});

test('desktop bridge serves capabilities and auth providers locally', async () => {
	const invokeCalls: InvokeCall[] = [];
	installBrowserEnv({
		origin: 'tauri://localhost',
		tauriRuntime: true,
		invoke: installInvokeProbe(invokeCalls),
	});

	const capabilities = await getApiCapabilities();
	const providers = await getAuthProviders();

	assert.equal(invokeCalls[0]?.cmd, 'desktop_get_contract_version');
	assert.equal(invokeCalls[1]?.cmd, 'desktop_get_capabilities');
	assert.equal(invokeCalls[2]?.cmd, 'desktop_get_contract_version');
	assert.equal(invokeCalls[3]?.cmd, 'desktop_get_auth_providers');
	assert.equal(capabilities.auth_enabled, false);
	assert.equal(providers.email_password, false);
});

test('desktop bridge lists repos via invoke bridge', async () => {
	const invokeCalls: InvokeCall[] = [];
	installBrowserEnv({
		origin: 'tauri://localhost',
		tauriRuntime: true,
		invoke: installInvokeProbe(invokeCalls),
	});

	const repos = await listSessionRepos();
	assert.deepEqual(repos.repos, ['acme/api', 'acme/web']);
	assert.equal(invokeCalls[0]?.cmd, 'desktop_get_contract_version');
	assert.equal(invokeCalls[1]?.cmd, 'desktop_list_repos');
});

test('desktop runtime builds handoff artifact via invoke bridge', async () => {
	const invokeCalls: InvokeCall[] = [];
	installBrowserEnv({
		origin: 'tauri://localhost',
		tauriRuntime: true,
		invoke: installInvokeProbe(invokeCalls),
	});

	const response = await buildSessionHandoff('session-1');
	assert.equal(response.artifact_uri, 'os://artifact/test123');
	assert.equal(response.pinned_alias, 'latest');
	assert.equal(invokeCalls[0]?.cmd, 'desktop_get_contract_version');
	assert.equal(invokeCalls[1]?.cmd, 'desktop_build_handoff');
	assert.deepEqual(invokeCalls[1]?.args, {
		request: { session_id: 'session-1', pin_latest: true },
	});
});

test('web runtime handoff build returns unsupported error', async () => {
	installBrowserEnv({ origin: 'http://127.0.0.1:5173' });
	installFetchProbe([]);

	await assert.rejects(
		() => buildSessionHandoff('session-1'),
		(error: unknown) =>
			error instanceof ApiError &&
			error.status === 501 &&
			error.code === 'desktop_handoff_unsupported',
	);
});

test('runtime override wins over desktop bridge', async () => {
	const invokeCalls: InvokeCall[] = [];
	installBrowserEnv({
		origin: 'tauri://localhost',
		tauriRuntime: true,
		runtimeOverride: 'http://localhost:3333',
		invoke: installInvokeProbe(invokeCalls),
	});
	const calls: string[] = [];
	installFetchProbe(calls);

	await listSessions();

	assert.equal(invokeCalls.length, 0);
	assert.equal(calls[0], 'http://localhost:3333/api/sessions');
});

test('setBaseUrl updates persisted URL used by API client', async () => {
	installBrowserEnv({ origin: 'http://127.0.0.1:5173' });
	const calls: string[] = [];
	installFetchProbe(calls);

	setBaseUrl('http://localhost:4200');
	await listSessions();

	assert.equal(calls[0], 'http://localhost:4200/api/sessions');
});
