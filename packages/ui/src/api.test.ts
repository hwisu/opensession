import assert from 'node:assert/strict';
import test from 'node:test';
import {
	ApiError,
	authLogin,
	authRegister,
	buildSessionHandoff,
	getApiCapabilities,
	getAuthProviders,
	getOAuthUrl,
	getRuntimeSettings,
	getSettings,
	getSessionDetail,
	listSessionRepos,
	listSessions,
	searchSessionsVector,
	setBaseUrl,
	updateRuntimeSettings,
	vectorIndexRebuild,
	vectorIndexStatus,
	vectorInstallModel,
	vectorPreflight,
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
	let runtimeSettings = {
		session_default_view: 'full',
		summary: {
			provider: {
				id: 'disabled',
				transport: 'none',
				endpoint: '',
				model: '',
			},
			prompt: {
				template: 'HAIL_COMPACT={{HAIL_COMPACT}}',
				default_template: 'HAIL_COMPACT={{HAIL_COMPACT}}',
			},
			response: {
				style: 'standard',
				shape: 'layered',
			},
			storage: {
				trigger: 'on_session_save',
				backend: 'hidden_ref',
			},
			source_mode: 'session_only',
		},
		vector_search: {
			enabled: false,
			provider: 'ollama',
			model: 'bge-m3',
			endpoint: 'http://127.0.0.1:11434',
			granularity: 'event_line_chunk',
			chunk_size_lines: 12,
			chunk_overlap_lines: 3,
			top_k_chunks: 30,
			top_k_sessions: 20,
		},
		ui_constraints: {
			source_mode_locked: true,
			source_mode_locked_value: 'session_only',
		},
	};

	return async (cmd: string, args?: Record<string, unknown>): Promise<unknown> => {
		calls.push({ cmd, args });
		switch (cmd) {
				case 'desktop_get_contract_version':
					return { version: 'desktop-ipc-v3' };
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
					download_file_name: 'handoff-test123.jsonl',
					download_content: '{"source_session_id":"session-1"}\n',
				};
			case 'desktop_get_runtime_settings':
				return runtimeSettings;
			case 'desktop_update_runtime_settings': {
				const request = (args?.request ?? {}) as {
					session_default_view?: string;
					summary?: {
						provider: { id: string; endpoint: string; model: string };
						prompt: { template: string };
						response: { style: string; shape: string };
						storage: { trigger: string; backend: string };
						source_mode: string;
					};
					vector_search?: {
						enabled: boolean;
						provider: string;
						model: string;
						endpoint: string;
						granularity: string;
						chunk_size_lines: number;
						chunk_overlap_lines: number;
						top_k_chunks: number;
						top_k_sessions: number;
					};
				};
				if (request.summary?.source_mode && request.summary.source_mode !== 'session_only') {
					throw {
						code: 'desktop.runtime_settings_source_mode_locked',
						status: 422,
						message: 'desktop source_mode is locked to session_only',
					};
				}
				runtimeSettings = {
					...runtimeSettings,
					session_default_view:
						request.session_default_view ?? runtimeSettings.session_default_view,
					summary: request.summary
						? {
								provider: {
									id: request.summary.provider.id,
									transport:
										request.summary.provider.id === 'ollama'
											? 'http'
											: request.summary.provider.id === 'disabled'
												? 'none'
												: 'cli',
									endpoint: request.summary.provider.endpoint,
									model: request.summary.provider.model,
								},
								prompt: {
									template: request.summary.prompt.template,
									default_template: runtimeSettings.summary.prompt.default_template,
								},
								response: request.summary.response,
								storage: request.summary.storage,
								source_mode: request.summary.source_mode,
							}
						: runtimeSettings.summary,
					vector_search: request.vector_search
						? {
								enabled: request.vector_search.enabled,
								provider: request.vector_search.provider,
								model: request.vector_search.model,
								endpoint: request.vector_search.endpoint,
								granularity: request.vector_search.granularity,
								chunk_size_lines: request.vector_search.chunk_size_lines,
								chunk_overlap_lines: request.vector_search.chunk_overlap_lines,
								top_k_chunks: request.vector_search.top_k_chunks,
								top_k_sessions: request.vector_search.top_k_sessions,
							}
						: runtimeSettings.vector_search,
				};
				return runtimeSettings;
			}
			case 'desktop_vector_preflight':
				return {
					provider: 'ollama',
					endpoint: runtimeSettings.vector_search.endpoint,
					model: runtimeSettings.vector_search.model,
					ollama_reachable: true,
					model_installed: true,
					install_state: 'ready',
					progress_pct: 100,
					message: 'ready',
				};
			case 'desktop_vector_install_model':
				return {
					state: 'installing',
					model: (args?.model as string | undefined) ?? runtimeSettings.vector_search.model,
					progress_pct: 0,
					message: 'starting model download',
				};
			case 'desktop_vector_index_rebuild':
			case 'desktop_vector_index_status':
				return {
					state: 'complete',
					processed_sessions: 10,
					total_sessions: 10,
					message: 'vector indexing complete',
					started_at: '2026-03-05T00:00:00Z',
					finished_at: '2026-03-05T00:01:00Z',
				};
			case 'desktop_search_sessions_vector':
				return {
					query: args?.query ?? '',
					sessions: [],
					next_cursor: null,
					total_candidates: 0,
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

test('desktop runtime without invoke bridge fails fast instead of local server fallback', async () => {
	installBrowserEnv({ origin: 'tauri://localhost', tauriRuntime: true });
	const calls: string[] = [];
	installFetchProbe(calls);

	await assert.rejects(
		() => listSessions(),
		(error: unknown) =>
			error instanceof ApiError &&
			error.status === 501 &&
			error.code === 'desktop_bridge_unavailable',
	);

	assert.equal(calls.length, 0);
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
	assert.equal(response.download_file_name, 'handoff-test123.jsonl');
	assert.match(response.download_content ?? '', /source_session_id/);
	assert.equal(invokeCalls[0]?.cmd, 'desktop_get_contract_version');
	assert.equal(invokeCalls[1]?.cmd, 'desktop_build_handoff');
	assert.deepEqual(invokeCalls[1]?.args, {
		request: { session_id: 'session-1', pin_latest: true },
	});
});

test('desktop runtime settings use typed payload through invoke bridge', async () => {
	const invokeCalls: InvokeCall[] = [];
	installBrowserEnv({
		origin: 'tauri://localhost',
		tauriRuntime: true,
		invoke: installInvokeProbe(invokeCalls),
	});

	const before = await getRuntimeSettings();
	assert.equal(before.summary.provider.id, 'disabled');

	const after = await updateRuntimeSettings({
		session_default_view: 'compressed',
		summary: {
			provider: {
				id: 'ollama',
				endpoint: 'http://127.0.0.1:11434',
				model: 'llama3.2:3b',
			},
			prompt: {
				template: 'Use {{HAIL_COMPACT}} only',
			},
			response: {
				style: 'compact',
				shape: 'layered',
			},
			storage: {
				trigger: 'on_session_save',
				backend: 'hidden_ref',
			},
			source_mode: 'session_only',
		},
	});
	assert.equal(after.session_default_view, 'compressed');
	assert.equal(after.summary.provider.id, 'ollama');
	assert.equal(after.summary.provider.transport, 'http');
	assert.equal(after.summary.prompt.template, 'Use {{HAIL_COMPACT}} only');
	assert.equal(after.summary.source_mode, 'session_only');
	assert.equal(invokeCalls.at(-1)?.cmd, 'desktop_update_runtime_settings');
});

test('desktop runtime settings surfaces source-mode lock errors', async () => {
	const invokeCalls: InvokeCall[] = [];
	installBrowserEnv({
		origin: 'tauri://localhost',
		tauriRuntime: true,
		invoke: installInvokeProbe(invokeCalls),
	});

	await assert.rejects(
		() =>
			updateRuntimeSettings({
				summary: {
					provider: {
						id: 'disabled',
						endpoint: '',
						model: '',
					},
					prompt: {
						template: '{{HAIL_COMPACT}}',
					},
					response: {
						style: 'standard',
						shape: 'layered',
					},
					storage: {
						trigger: 'on_session_save',
						backend: 'hidden_ref',
					},
					source_mode: 'session_or_git_changes',
				},
			}),
		(error: unknown) =>
			error instanceof ApiError &&
			error.status === 422 &&
			error.code === 'desktop.runtime_settings_source_mode_locked',
	);
	assert.equal(invokeCalls.at(-1)?.cmd, 'desktop_update_runtime_settings');
});

test('desktop vector controls use invoke bridge', async () => {
	const invokeCalls: InvokeCall[] = [];
	installBrowserEnv({
		origin: 'tauri://localhost',
		tauriRuntime: true,
		invoke: installInvokeProbe(invokeCalls),
	});

	const preflight = await vectorPreflight();
	assert.equal(preflight.model, 'bge-m3');
	assert.equal(preflight.install_state, 'ready');

	const install = await vectorInstallModel('bge-m3');
	assert.equal(install.state, 'installing');

	const rebuild = await vectorIndexRebuild();
	assert.equal(rebuild.state, 'complete');

	const status = await vectorIndexStatus();
	assert.equal(status.state, 'complete');

	const search = await searchSessionsVector('auth parser retry', null, 20);
	assert.equal(search.total_candidates, 0);

	const calledCommands = invokeCalls.map((entry) => entry.cmd);
	assert(calledCommands.includes('desktop_vector_preflight'));
	assert(calledCommands.includes('desktop_vector_install_model'));
	assert(calledCommands.includes('desktop_vector_index_rebuild'));
	assert(calledCommands.includes('desktop_vector_index_status'));
	assert(calledCommands.includes('desktop_search_sessions_vector'));
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

test('desktop local runtime blocks HTTP auth endpoints without API override', async () => {
	const invokeCalls: InvokeCall[] = [];
	installBrowserEnv({
		origin: 'tauri://localhost',
		tauriRuntime: true,
		invoke: installInvokeProbe(invokeCalls),
	});
	const fetchCalls: string[] = [];
	installFetchProbe(fetchCalls);

	await assert.rejects(
		() => getSettings(),
		(error: unknown) =>
			error instanceof ApiError &&
			error.status === 501 &&
			error.code === 'desktop_http_api_unavailable',
	);

	assert.equal(fetchCalls.length, 0);
	assert.equal(invokeCalls.length, 0);
});

test('desktop local runtime blocks auth login/register HTTP endpoints without API override', async () => {
	const invokeCalls: InvokeCall[] = [];
	installBrowserEnv({
		origin: 'tauri://localhost',
		tauriRuntime: true,
		invoke: installInvokeProbe(invokeCalls),
	});
	const fetchCalls: string[] = [];
	installFetchProbe(fetchCalls);

	await assert.rejects(
		() => authLogin('dev@opensession.local', 'pw'),
		(error: unknown) =>
			error instanceof ApiError &&
			error.status === 501 &&
			error.code === 'desktop_http_api_unavailable',
	);

	await assert.rejects(
		() => authRegister('dev@opensession.local', 'pw', 'dev'),
		(error: unknown) =>
			error instanceof ApiError &&
			error.status === 501 &&
			error.code === 'desktop_http_api_unavailable',
	);

	assert.equal(fetchCalls.length, 0);
	assert.equal(invokeCalls.length, 0);
});

test('desktop local runtime returns inert oauth URL without API override', async () => {
	installBrowserEnv({
		origin: 'tauri://localhost',
		tauriRuntime: true,
		invoke: installInvokeProbe([]),
	});

	assert.equal(getOAuthUrl('github'), '#');
});

test('desktop runtime with API override uses HTTP auth endpoints', async () => {
	const invokeCalls: InvokeCall[] = [];
	installBrowserEnv({
		origin: 'tauri://localhost',
		tauriRuntime: true,
		runtimeOverride: 'http://localhost:3333',
		invoke: installInvokeProbe(invokeCalls),
	});
	const fetchCalls: string[] = [];
	installFetchProbe(fetchCalls, {
		user_id: 'override-user',
		nickname: 'override-nick',
		created_at: '2026-03-05T00:00:00Z',
		email: null,
		avatar_url: null,
		oauth_providers: [],
	});

	await getSettings();
	assert.equal(fetchCalls[0], 'http://localhost:3333/api/auth/me');
	assert.equal(invokeCalls.length, 0);
	assert.equal(
		getOAuthUrl('github'),
		'http://localhost:3333/api/auth/oauth/github',
	);
});

test('setBaseUrl updates persisted URL used by API client', async () => {
	installBrowserEnv({ origin: 'http://127.0.0.1:5173' });
	const calls: string[] = [];
	installFetchProbe(calls);

	setBaseUrl('http://localhost:4200');
	await listSessions();

	assert.equal(calls[0], 'http://localhost:4200/api/sessions');
});
