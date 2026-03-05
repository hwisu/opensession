import { expect, test } from '@playwright/test';

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000';

test.describe('Settings', () => {
	test('settings page requires auth when no token exists', async ({ page }) => {
		await page.route('**/api/capabilities', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					auth_enabled: true,
					parse_preview_enabled: true,
					register_targets: ['local', 'git'],
					share_modes: ['web', 'git', 'json'],
				}),
			});
		});
		await page.goto('/settings');
		await expect(page.locator('[data-testid="settings-require-auth"]')).toBeVisible();
		await expect(page.locator('[data-testid="settings-require-auth"]')).toContainText(
			'Sign in is required',
		);
	});

	test('desktop runtime without auth API hides account UI and skips auth endpoints', async ({
		page,
	}) => {
		await page.addInitScript(() => {
			(window as Window & { __TAURI_INTERNALS__?: Record<string, never> }).__TAURI_INTERNALS__ = {};
		});

		let authCalls = 0;
		await page.route('**/api/capabilities', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					auth_enabled: false,
					parse_preview_enabled: false,
					register_targets: [],
					share_modes: [],
				}),
			});
		});
		await page.route('**/api/auth/**', async (route) => {
			authCalls += 1;
			await route.fulfill({
				status: 500,
				contentType: 'application/json',
				body: JSON.stringify({ code: 'unexpected_auth_call', message: 'should not be called' }),
			});
		});

		await page.goto('/settings');
		await expect(page.locator('[data-testid="settings-auth-disabled"]')).toHaveCount(0);
		await expect(page.locator('[data-testid="settings-require-auth"]')).toHaveCount(0);
		await expect.poll(() => authCalls).toBe(0);
	});

	test('desktop runtime settings renders provider rules, prompt reset, and deterministic preview', async ({
		page,
	}) => {
		await page.addInitScript(() => {
			type SummaryProviderId = 'disabled' | 'ollama' | 'codex_exec' | 'claude_cli';
			const transportFor = (provider: SummaryProviderId): 'none' | 'http' | 'cli' => {
				if (provider === 'ollama') return 'http';
				if (provider === 'codex_exec' || provider === 'claude_cli') return 'cli';
				return 'none';
			};
			const defaultTemplate =
				'Convert a real coding session into semantic compression.\\nHAIL_COMPACT={{HAIL_COMPACT}}';
			let runtimeState = {
				session_default_view: 'full',
				summary: {
					provider: {
						id: 'disabled' as SummaryProviderId,
						transport: 'none' as 'none' | 'http' | 'cli',
						endpoint: '',
						model: '',
					},
					prompt: {
						template: defaultTemplate,
						default_template: defaultTemplate,
					},
					response: {
						style: 'standard' as 'compact' | 'standard' | 'detailed',
						shape: 'layered' as 'layered' | 'file_list' | 'security_first',
					},
					storage: {
						trigger: 'on_session_save' as 'manual' | 'on_session_save',
						backend: 'hidden_ref' as 'hidden_ref' | 'local_db' | 'none',
					},
					source_mode: 'session_only' as 'session_only' | 'session_or_git_changes',
				},
				vector_search: {
					enabled: false,
					provider: 'ollama' as 'ollama',
					model: 'bge-m3',
					endpoint: 'http://127.0.0.1:11434',
					granularity: 'event_line_chunk' as 'event_line_chunk',
					chunk_size_lines: 12,
					chunk_overlap_lines: 3,
					top_k_chunks: 30,
					top_k_sessions: 20,
				},
				change_reader: {
					enabled: false,
					scope: 'summary_only' as 'summary_only' | 'full_context',
					qa_enabled: true,
					max_context_chars: 12000,
				},
				ui_constraints: {
					source_mode_locked: true,
					source_mode_locked_value: 'session_only' as 'session_only',
				},
			};

			(
				window as Window & {
					__TAURI_INTERNALS__?: Record<string, never>;
					__TAURI__?: { core?: { invoke?: (cmd: string, args?: Record<string, unknown>) => Promise<unknown> } };
				}
			).__TAURI_INTERNALS__ = {};
			(window as Window & { __TAURI__?: unknown }).__TAURI__ = {
				core: {
					invoke: async (cmd: string, args?: Record<string, unknown>) => {
						if (cmd === 'desktop_get_contract_version') return { version: 'desktop-ipc-v4' };
						if (cmd === 'desktop_get_capabilities') {
							return {
								auth_enabled: false,
								parse_preview_enabled: false,
								register_targets: [],
								share_modes: [],
							};
						}
						if (cmd === 'desktop_get_auth_providers') {
							return { email_password: false, oauth: [] };
						}
						if (cmd === 'desktop_get_runtime_settings') return runtimeState;
						if (cmd === 'desktop_vector_preflight') {
							return {
								provider: 'ollama',
								endpoint: runtimeState.vector_search.endpoint,
								model: runtimeState.vector_search.model,
								ollama_reachable: true,
								model_installed: true,
								install_state: 'ready',
								progress_pct: 100,
								message: 'ready',
							};
						}
						if (cmd === 'desktop_vector_index_status' || cmd === 'desktop_vector_index_rebuild') {
							return {
								state: 'complete',
								processed_sessions: 5,
								total_sessions: 5,
								message: 'vector indexing complete',
								started_at: '2026-03-05T00:00:00Z',
								finished_at: '2026-03-05T00:00:05Z',
							};
						}
						if (cmd === 'desktop_vector_install_model') {
							return {
								state: 'installing',
								model: (args?.model as string | undefined) ?? runtimeState.vector_search.model,
								progress_pct: 0,
								message: 'starting model download',
							};
						}
						if (cmd === 'desktop_search_sessions_vector') {
							return {
								query: (args?.query as string | undefined) ?? '',
								sessions: [],
								next_cursor: null,
								total_candidates: 0,
							};
						}
						if (cmd === 'desktop_detect_summary_provider') {
							return {
								detected: true,
								provider: 'codex_exec',
								transport: 'cli',
								model: '',
								endpoint: '',
							};
						}
						if (cmd === 'desktop_update_runtime_settings') {
							const request = (args?.request ?? {}) as {
								session_default_view?: string;
								summary?: {
									provider: { id: SummaryProviderId; endpoint: string; model: string };
									prompt: { template: string };
									response: { style: 'compact' | 'standard' | 'detailed'; shape: 'layered' | 'file_list' | 'security_first' };
									storage: { trigger: 'manual' | 'on_session_save'; backend: 'hidden_ref' | 'local_db' | 'none' };
									source_mode: 'session_only' | 'session_or_git_changes';
								};
								vector_search?: {
									enabled: boolean;
									provider: 'ollama';
									model: string;
									endpoint: string;
									granularity: 'event_line_chunk';
									chunk_size_lines: number;
									chunk_overlap_lines: number;
									top_k_chunks: number;
									top_k_sessions: number;
								};
								change_reader?: {
									enabled: boolean;
									scope: 'summary_only' | 'full_context';
									qa_enabled: boolean;
									max_context_chars: number;
								};
							};
							if (request.summary && request.summary.source_mode !== 'session_only') {
								throw {
									code: 'desktop.runtime_settings_source_mode_locked',
									status: 422,
									message: 'desktop source_mode is locked to session_only',
								};
							}
							runtimeState = {
								...runtimeState,
								session_default_view:
									request.session_default_view ?? runtimeState.session_default_view,
								summary: request.summary
									? {
											provider: {
												id: request.summary.provider.id,
												transport: transportFor(request.summary.provider.id),
												endpoint: request.summary.provider.endpoint,
												model: request.summary.provider.model,
											},
											prompt: {
												template: request.summary.prompt.template,
												default_template: runtimeState.summary.prompt.default_template,
											},
											response: request.summary.response,
											storage: request.summary.storage,
											source_mode: request.summary.source_mode,
										}
									: runtimeState.summary,
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
									: runtimeState.vector_search,
								change_reader: request.change_reader
									? {
											enabled: request.change_reader.enabled,
											scope: request.change_reader.scope,
											qa_enabled: request.change_reader.qa_enabled,
											max_context_chars: request.change_reader.max_context_chars,
										}
									: runtimeState.change_reader,
							};
							return runtimeState;
						}
						throw new Error(`unexpected command: ${cmd}`);
					},
				},
			};
		});

		await page.goto('/settings');
		await expect(page.locator('[data-testid="settings-runtime-provider"]')).toBeVisible();
		await expect(page.locator('option[value="session_or_git_changes"]')).toHaveCount(0);

		await page.locator('[data-testid="runtime-provider-select"]').selectOption('ollama');
		await expect(page.locator('[data-testid="runtime-provider-endpoint"]')).toBeVisible();

		await page.locator('[data-testid="runtime-provider-select"]').selectOption('codex_exec');
		await expect(page.locator('[data-testid="runtime-provider-endpoint"]')).toHaveCount(0);
		await expect(page.locator('[data-testid="runtime-provider-cli-status"]')).toBeVisible();

		await page.locator('[data-testid="runtime-prompt-template"]').fill('custom {{HAIL_COMPACT}}');
		await page.locator('[data-testid="runtime-prompt-reset-default"]').click();
		await expect(page.locator('[data-testid="runtime-prompt-template"]')).toHaveValue(
			/HAIL_COMPACT/,
		);

		const preview = page.locator('[data-testid="settings-response-preview"] pre');
		await expect(preview).toContainText('Runtime settings and summary');
		await page
			.locator('[data-testid="settings-runtime-response"] select')
			.first()
			.selectOption('compact');
		await expect(preview).toContainText('Updated session summary pipeline');

		await expect(page.locator('[data-testid="settings-runtime-vector"]')).toBeVisible();
		await expect(page.locator('[data-testid="runtime-vector-status"]')).toContainText(
			'install_state: ready',
		);
		await expect(page.locator('[data-testid="settings-runtime-change-reader"]')).toBeVisible();
		await page.locator('[data-testid="runtime-change-reader-enable"]').check();
		await page
			.locator('[data-testid="settings-runtime-change-reader"] select')
			.first()
			.selectOption('full_context');
		await page.locator('[data-testid="runtime-change-reader-max-context"]').fill('18000');
	});

	test('can issue personal api key from settings page', async ({ page }) => {
		const secure = BASE_URL.startsWith('https://');
		const domain = new URL(BASE_URL).hostname;
		const now = Math.floor(Date.now() / 1000);
		await page.context().addCookies([
			{
				name: 'opensession_csrf_token',
				value: 'settings-csrf-token',
				domain,
				path: '/',
				httpOnly: false,
				secure,
				sameSite: 'Lax',
				expires: now + 3600,
			},
		]);

		await page.route('**/api/capabilities', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					auth_enabled: true,
					parse_preview_enabled: true,
					register_targets: ['local', 'git'],
					share_modes: ['web', 'git', 'json'],
				}),
			});
		});

		await page.route('**/api/auth/verify', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({ user_id: 'u-settings', nickname: 'settings-user' }),
			});
		});

		await page.route('**/api/auth/me', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					user_id: 'u-settings',
					nickname: 'settings-user',
					created_at: '2026-02-21T00:00:00.000Z',
					email: 'settings@test.local',
					avatar_url: null,
					oauth_providers: [
						{ provider: 'github', provider_username: 'settings-user', display_name: 'GitHub' },
					],
				}),
			});
		});

		const credentials: Array<{
			id: string;
			label: string;
			host: string;
			path_prefix: string;
			header_name: string;
			created_at: string;
			updated_at: string;
			last_used_at: string | null;
		}> = [
			{
				id: 'cred-1',
				label: 'GitLab Internal',
				host: 'gitlab.internal.example.com',
				path_prefix: 'group/subgroup',
				header_name: 'Authorization',
				created_at: '2026-02-21T00:00:00.000Z',
				updated_at: '2026-02-21T00:00:00.000Z',
				last_used_at: null,
			},
		];
		await page.route('**/api/auth/git-credentials', async (route) => {
			if (route.request().method() === 'GET') {
				await route.fulfill({
					status: 200,
					contentType: 'application/json',
					body: JSON.stringify({ credentials }),
				});
				return;
			}
			if (route.request().method() === 'POST') {
				const body = route.request().postDataJSON() as {
					label?: string;
					host?: string;
					path_prefix?: string | null;
					header_name?: string;
				};
				const created = {
					id: `cred-${credentials.length + 1}`,
					label: body.label ?? 'unknown',
					host: body.host ?? 'unknown',
					path_prefix: body.path_prefix ?? '',
					header_name: body.header_name ?? 'Authorization',
					created_at: '2026-02-22T00:00:00.000Z',
					updated_at: '2026-02-22T00:00:00.000Z',
					last_used_at: null,
				};
				credentials.push(created);
				await route.fulfill({
					status: 201,
					contentType: 'application/json',
					body: JSON.stringify(created),
				});
				return;
			}
			await route.fallback();
		});
		await page.route('**/api/auth/git-credentials/*', async (route) => {
			if (route.request().method() !== 'DELETE') {
				await route.fallback();
				return;
			}
			const id = route.request().url().split('/').pop() ?? '';
			const decoded = decodeURIComponent(id);
			const index = credentials.findIndex((credential) => credential.id === decoded);
			if (index >= 0) {
				credentials.splice(index, 1);
				await route.fulfill({
					status: 200,
					contentType: 'application/json',
					body: JSON.stringify({ ok: true }),
				});
				return;
			}
			await route.fulfill({
				status: 404,
				contentType: 'application/json',
				body: JSON.stringify({ code: 'not_found', message: 'credential not found' }),
			});
		});

		await page.route('**/api/auth/api-keys/issue', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({ api_key: 'osk_settings_e2e_test_key' }),
			});
		});

		await page.goto('/settings');
		await expect(page.locator('[data-testid="settings-page"]')).toBeVisible();
		await expect(page.locator('[data-testid="settings-page"]')).toContainText('settings-user');
		await expect(page.locator('[data-testid="settings-page"]')).toContainText('settings@test.local');
		await expect(page.locator('[data-testid="git-credential-settings"]')).toContainText(
			'GitLab Internal',
		);

		await page.locator('[data-testid="issue-api-key-button"]').click();
		await expect(page.locator('[data-testid="issued-api-key"]')).toContainText(
			'osk_settings_e2e_test_key',
		);
		await expect(page.locator('[data-testid="copy-api-key"]')).toBeVisible();

		await page.locator('[data-testid="git-credential-label"]').fill('Generic Git');
		await page.locator('[data-testid="git-credential-host"]').fill('code.example.com');
		await page.locator('[data-testid="git-credential-path-prefix"]').fill('team/repo');
		await page.locator('[data-testid="git-credential-header-name"]').fill('Authorization');
		await page.locator('[data-testid="git-credential-header-value"]').fill('Bearer abc123');
		await page.locator('[data-testid="git-credential-save"]').click();
		await expect(page.locator('[data-testid="git-credential-settings"]')).toContainText('Generic Git');

		await page.locator('[data-testid="git-credential-delete-cred-1"]').click();
		await expect(page.locator('[data-testid="git-credential-settings"]')).not.toContainText(
			'GitLab Internal',
		);
	});
});
