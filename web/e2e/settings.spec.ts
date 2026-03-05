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
				batch: {
					execution_mode: 'on_app_start' as 'manual' | 'on_app_start',
					scope: 'recent_days' as 'recent_days' | 'all',
					recent_days: 30,
				},
			},
				vector_search: {
					enabled: false,
					provider: 'ollama' as 'ollama',
					model: 'bge-m3',
					endpoint: 'http://127.0.0.1:11434',
					granularity: 'event_line_chunk' as 'event_line_chunk',
					chunking_mode: 'auto' as 'auto' | 'manual',
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
				voice: {
					enabled: false,
					provider: 'openai' as 'openai',
					model: 'gpt-4o-mini-tts',
					voice: 'alloy',
					api_key_configured: false,
				},
			},
			lifecycle: {
				enabled: true,
				session_ttl_days: 30,
				summary_ttl_days: 30,
				cleanup_interval_secs: 3600,
			},
			ui_constraints: {
				source_mode_locked: true,
				source_mode_locked_value: 'session_only' as 'session_only',
			},
		};
		let summaryBatchStatus = {
			state: 'idle' as 'idle' | 'running' | 'complete' | 'failed',
			processed_sessions: 0,
			total_sessions: 0,
			failed_sessions: 0,
			message: null as string | null,
			started_at: null as string | null,
			finished_at: null as string | null,
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
						if (cmd === 'desktop_get_contract_version') return { version: 'desktop-ipc-v6' };
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
									batch: { execution_mode: 'manual' | 'on_app_start'; scope: 'recent_days' | 'all'; recent_days: number };
								};
								vector_search?: {
									enabled: boolean;
									provider: 'ollama';
									model: string;
									endpoint: string;
									granularity: 'event_line_chunk';
									chunking_mode: 'auto' | 'manual';
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
									voice: {
										enabled: boolean;
										provider: 'openai';
										model: string;
										voice: string;
										api_key?: string | null;
									};
								};
								lifecycle?: {
									enabled: boolean;
									session_ttl_days: number;
									summary_ttl_days: number;
									cleanup_interval_secs: number;
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
											batch: request.summary.batch,
										}
									: runtimeState.summary,
								vector_search: request.vector_search
									? {
											enabled: request.vector_search.enabled,
											provider: request.vector_search.provider,
											model: request.vector_search.model,
											endpoint: request.vector_search.endpoint,
											granularity: request.vector_search.granularity,
											chunking_mode: request.vector_search.chunking_mode,
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
											voice: {
												enabled: request.change_reader.voice.enabled,
												provider: request.change_reader.voice.provider,
												model: request.change_reader.voice.model,
												voice: request.change_reader.voice.voice,
												api_key_configured:
													typeof request.change_reader.voice.api_key === 'string'
														? request.change_reader.voice.api_key.trim().length > 0
														: runtimeState.change_reader.voice.api_key_configured,
											},
										}
									: runtimeState.change_reader,
								lifecycle: request.lifecycle
									? {
											enabled: request.lifecycle.enabled,
											session_ttl_days: request.lifecycle.session_ttl_days,
											summary_ttl_days: request.lifecycle.summary_ttl_days,
											cleanup_interval_secs: request.lifecycle.cleanup_interval_secs,
										}
									: runtimeState.lifecycle,
							};
							return runtimeState;
						}
						if (cmd === 'desktop_summary_batch_run') {
							summaryBatchStatus = {
								state: 'running',
								processed_sessions: 0,
								total_sessions: 4,
								failed_sessions: 0,
								message: 'processing semantic summaries',
								started_at: '2026-03-05T00:00:00Z',
								finished_at: null,
							};
							return summaryBatchStatus;
						}
						if (cmd === 'desktop_summary_batch_status') {
							if (summaryBatchStatus.state === 'running') {
								summaryBatchStatus = {
									...summaryBatchStatus,
									state: 'complete',
									processed_sessions: 4,
									total_sessions: 4,
									failed_sessions: 0,
									message: 'summary batch complete',
									finished_at: '2026-03-05T00:00:05Z',
								};
							}
							return summaryBatchStatus;
						}
						throw new Error(`unexpected command: ${cmd}`);
					},
				},
			};
		});

		await page.goto('/settings');
		await expect(page.locator('[data-testid="settings-runtime-provider"]')).toBeVisible();
		await expect(page.locator('option[value="session_or_git_changes"]')).toHaveCount(0);
		const detectProviderButton = page.locator('[data-testid="runtime-detect-provider"]');
		const saveRuntimeButton = page.locator('[data-testid="runtime-save"]');
		const detectProviderBox = await detectProviderButton.boundingBox();
		const saveRuntimeBox = await saveRuntimeButton.boundingBox();
		expect(detectProviderBox).not.toBeNull();
		expect(saveRuntimeBox).not.toBeNull();
		expect(Math.abs((detectProviderBox?.height ?? 0) - (saveRuntimeBox?.height ?? 0))).toBeLessThanOrEqual(2);

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
			'install ready (100%)',
		);
		await expect(page.locator('[data-testid="runtime-vector-status"]')).toContainText(
			'Vector pipeline is ready.',
		);
		await expect(page.locator('[data-testid="runtime-vector-chunking-mode"]')).toHaveValue('auto');
		await expect(page.locator('[data-testid="runtime-vector-chunk-size"]')).toBeDisabled();
		await page.locator('[data-testid="runtime-vector-chunking-mode"]').selectOption('manual');
		await expect(page.locator('[data-testid="runtime-vector-chunk-size"]')).toBeEnabled();
		const runtimeHelpHints = page.locator('[data-testid^="runtime-help-"]');
		expect(await runtimeHelpHints.count()).toBeGreaterThanOrEqual(20);
		await page.locator('[data-testid="runtime-help-storage-backend"]').hover();
		await expect(page.getByRole('tooltip')).toContainText('Where summary artifacts persist');
		await expect(page.locator('[data-testid="settings-runtime-change-reader"]')).toBeVisible();
		await page.locator('[data-testid="runtime-help-change-reader-enable"]').hover();
		await expect(page.getByRole('tooltip')).toContainText('notebook-style change reading');
		await page.locator('[data-testid="runtime-change-reader-enable"]').check();
		await page
			.locator('[data-testid="settings-runtime-change-reader"] select')
			.first()
			.selectOption('full_context');
		await page.locator('[data-testid="runtime-change-reader-max-context"]').fill('18000');
		await page.locator('[data-testid="runtime-change-reader-voice-enable"]').check();
		await expect(page.locator('[data-testid="runtime-change-reader-voice-provider"]')).toHaveValue(
			'openai',
		);
		await page.locator('[data-testid="runtime-change-reader-voice-model"]').fill('gpt-4o-mini-tts');
		await page.locator('[data-testid="runtime-change-reader-voice-name"]').fill('alloy');
		await page.locator('[data-testid="runtime-change-reader-voice-api-key"]').fill(
			'test-openai-key',
		);
		await expect(
			page.locator('[data-testid="runtime-change-reader-voice-key-status"]'),
		).toContainText('api_key_configured: no');

		await expect(page.locator('[data-testid="settings-runtime-storage"]')).toBeVisible();
		await expect(page.locator('[data-testid="runtime-storage-backend-notice"]')).toContainText(
			'Persist summary artifacts in git-native hidden refs',
		);
		await page.locator('[data-testid="settings-runtime-storage"] select').nth(1).selectOption('local_db');
		await expect(page.locator('[data-testid="runtime-storage-backend-notice"]')).toContainText(
			'session_semantic_summaries',
		);
		await page.locator('[data-testid="settings-runtime-storage"] select').nth(1).selectOption('none');
		await expect(page.locator('[data-testid="runtime-storage-backend-notice"]')).toContainText(
			'results are not stored',
		);

		await expect(page.locator('[data-testid="settings-runtime-summary-batch"]')).toBeVisible();
		await page
			.locator('[data-testid="settings-runtime-summary-batch"] select')
			.first()
			.selectOption('manual');
		await page
			.locator('[data-testid="settings-runtime-summary-batch"] select')
			.nth(1)
			.selectOption('recent_days');
		await page.locator('[data-testid="runtime-summary-batch-recent-days"]').fill('14');
		await page.locator('[data-testid="runtime-summary-batch-run"]').click();
		await expect(page.locator('[data-testid="runtime-summary-batch-status"]')).toContainText(
			'state: complete',
		);

		await expect(page.locator('[data-testid="settings-runtime-lifecycle"]')).toBeVisible();
		await page.locator('[data-testid="runtime-lifecycle-session-ttl"]').fill('45');
		await page.locator('[data-testid="runtime-lifecycle-summary-ttl"]').fill('60');
		await page.locator('[data-testid="runtime-lifecycle-interval"]').fill('120');
		await page.getByRole('button', { name: 'Save Runtime' }).click();
		await expect(page.locator('[data-testid="settings-runtime-lifecycle"]')).toContainText(
			'Enable periodic lifecycle cleanup',
		);
		await expect(
			page.locator('[data-testid="runtime-change-reader-voice-key-status"]'),
		).toContainText('api_key_configured: yes');
		await page.evaluate(() => {
			const active = document.activeElement as HTMLElement | null;
			active?.blur?.();
			(document.body ?? window).dispatchEvent(
				new KeyboardEvent('keydown', {
					key: '?',
					shiftKey: true,
					bubbles: true,
				}),
			);
		});
		await expect(page.locator('[data-testid="keyboard-help-modal"]')).toBeVisible();
		await page.keyboard.press('Escape');
		await expect(page.locator('[data-testid="keyboard-help-modal"]')).toHaveCount(0);
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

	test('vector status shows actionable errors when ollama is unavailable', async ({ page }) => {
		await page.addInitScript(() => {
			type SummaryProviderId = 'disabled' | 'ollama' | 'codex_exec' | 'claude_cli';
			const runtimeState = {
				session_default_view: 'full' as 'full' | 'compressed',
				summary: {
					provider: {
						id: 'disabled' as SummaryProviderId,
						transport: 'none' as 'none' | 'http' | 'cli',
						endpoint: '',
						model: '',
					},
					prompt: {
						template:
							'Convert a real coding session into semantic compression.\\nHAIL_COMPACT={{HAIL_COMPACT}}',
						default_template:
							'Convert a real coding session into semantic compression.\\nHAIL_COMPACT={{HAIL_COMPACT}}',
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
					batch: {
						execution_mode: 'on_app_start' as 'manual' | 'on_app_start',
						scope: 'recent_days' as 'recent_days' | 'all',
						recent_days: 30,
					},
				},
				vector_search: {
					enabled: true,
					provider: 'ollama' as 'ollama',
					model: 'bge-m3',
					endpoint: 'http://127.0.0.1:11434',
					granularity: 'event_line_chunk' as 'event_line_chunk',
					chunking_mode: 'auto' as 'auto' | 'manual',
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
					voice: {
						enabled: false,
						provider: 'openai' as 'openai',
						model: 'gpt-4o-mini-tts',
						voice: 'alloy',
						api_key_configured: false,
					},
				},
				lifecycle: {
					enabled: true,
					session_ttl_days: 30,
					summary_ttl_days: 30,
					cleanup_interval_secs: 3600,
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
						if (cmd === 'desktop_get_contract_version') return { version: 'desktop-ipc-v6' };
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
						if (cmd === 'desktop_summary_batch_status') {
							return {
								state: 'idle',
								processed_sessions: 0,
								total_sessions: 0,
								failed_sessions: 0,
								message: null,
								started_at: null,
								finished_at: null,
							};
						}
						if (cmd === 'desktop_vector_preflight') {
							return {
								provider: 'ollama',
								endpoint: runtimeState.vector_search.endpoint,
								model: runtimeState.vector_search.model,
								ollama_reachable: false,
								model_installed: false,
								install_state: 'failed',
								progress_pct: 0,
								message:
									'ollama CLI is not installed. Install from https://ollama.com/download, then run `ollama serve`.',
							};
						}
						if (cmd === 'desktop_vector_index_status') {
							return {
								state: 'idle',
								processed_sessions: 0,
								total_sessions: 0,
								message: null,
								started_at: null,
								finished_at: null,
							};
						}
						if (cmd === 'desktop_vector_install_model') {
							const model =
								(args?.model as string | undefined) ?? runtimeState.vector_search.model;
							throw {
								code: 'desktop.vector_install_unavailable',
								status: 422,
								message: 'ollama CLI is not installed',
								details: {
									model,
									endpoint: runtimeState.vector_search.endpoint,
									hint: 'install Ollama from https://ollama.com/download and run `ollama serve`',
								},
							};
						}
						if (cmd === 'desktop_update_runtime_settings') return runtimeState;
						if (cmd === 'desktop_detect_summary_provider') {
							return {
								detected: false,
								provider: null,
								transport: null,
								model: null,
								endpoint: null,
							};
						}
						if (cmd === 'desktop_summary_batch_run') {
							return {
								state: 'idle',
								processed_sessions: 0,
								total_sessions: 0,
								failed_sessions: 0,
								message: null,
								started_at: null,
								finished_at: null,
							};
						}
						if (cmd === 'desktop_vector_index_rebuild') {
							return {
								state: 'idle',
								processed_sessions: 0,
								total_sessions: 0,
								message: null,
								started_at: null,
								finished_at: null,
							};
						}
						throw new Error(`unexpected command: ${cmd}`);
					},
				},
			};
		});

		await page.goto('/settings');
		const vectorStatus = page.locator('[data-testid="runtime-vector-status"]');
		await expect(vectorStatus).toBeVisible();
		await expect(vectorStatus).toContainText('ollama CLI is not installed');
		await expect(vectorStatus).toContainText('Install Ollama: https://ollama.com/download');
		await expect(page.locator('[data-testid="runtime-vector-reindex"]')).toBeDisabled();

		await page.locator('[data-testid="runtime-vector-install"]').click();
		await expect(page.locator('[data-testid="runtime-vector-error"]')).toContainText(
			'Action: install Ollama from https://ollama.com/download and run `ollama serve`',
		);
	});
});
