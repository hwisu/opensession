import { expect, test } from '@playwright/test';

function buildPreviewResponse(overrides?: Partial<Record<string, unknown>>) {
	const now = new Date().toISOString();
	return {
		parser_used: 'codex',
		parser_candidates: [
			{ id: 'codex', confidence: 92, reason: 'fixture' },
			{ id: 'hail', confidence: 48, reason: 'fallback' },
		],
		session: {
			version: 'hail-1.0.0',
			session_id: 'src-fixture-1',
			agent: {
				provider: 'openai',
				model: 'gpt-5',
				tool: 'codex',
			},
			context: {
				title: 'Source Fixture',
				description: 'fixture session',
				tags: ['src', 'share'],
				created_at: now,
				updated_at: now,
				related_session_ids: [],
				attributes: {},
			},
			events: [
				{
					event_id: 'u1',
					timestamp: now,
					event_type: { type: 'UserMessage' },
					content: { blocks: [{ type: 'Text', text: 'hello from user' }] },
					attributes: {},
				},
				{
					event_id: 'a1',
					timestamp: now,
					event_type: { type: 'AgentMessage' },
					content: { blocks: [{ type: 'Text', text: 'hello from assistant' }] },
					attributes: {},
				},
			],
			stats: {
				event_count: 2,
				message_count: 2,
				tool_call_count: 0,
				task_count: 0,
				duration_seconds: 0,
				total_input_tokens: 0,
				total_output_tokens: 0,
				user_message_count: 1,
				files_changed: 0,
				lines_added: 0,
				lines_removed: 0,
			},
		},
		source: {
			kind: 'git',
			remote: 'https://github.com/hwisu/opensession',
			ref: 'main',
			path: 'sessions/demo.hail.jsonl',
		},
		warnings: [],
		native_adapter: 'codex',
		...overrides,
	};
}

function base64UrlEncode(value: string): string {
	return Buffer.from(value, 'utf8')
		.toString('base64')
		.replace(/\+/g, '-')
		.replace(/\//g, '_')
		.replace(/=+$/g, '');
}

function srcGitUrl(): string {
	const remote = base64UrlEncode('https://github.com/hwisu/opensession');
	return `/src/git/${remote}/ref/main/path/sessions/demo.hail.jsonl`;
}

test.describe('Source Route', () => {
	test('renders session automatically from /src/git path', async ({ page }) => {
		await page.route('**/api/capabilities', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					auth_enabled: false,
					parse_preview_enabled: true,
					register_targets: ['local', 'git'],
					share_modes: ['web', 'git', 'json'],
				}),
			});
		});
		await page.route('**/api/parse/preview', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify(buildPreviewResponse()),
			});
		});

		await page.goto(srcGitUrl());
		await expect(page.getByRole('heading', { name: 'Source Fixture' })).toBeVisible({ timeout: 10000 });
		await expect(page.getByText('hello from user')).toBeVisible();
		await expect(page.getByText('hello from assistant')).toBeVisible();
	});

	test('supports parser selection retry flow on /src', async ({ page }) => {
		await page.route('**/api/capabilities', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					auth_enabled: false,
					parse_preview_enabled: true,
					register_targets: ['local', 'git'],
					share_modes: ['web', 'git', 'json'],
				}),
			});
		});
		await page.route('**/api/parse/preview', async (route) => {
			const request = route.request();
			const body = request.postDataJSON() as { parser_hint?: string };
			if (!body.parser_hint) {
				await route.fulfill({
					status: 422,
					contentType: 'application/json',
					body: JSON.stringify({
						code: 'parser_selection_required',
						message: 'choose parser',
						parser_candidates: [
							{ id: 'codex', confidence: 92, reason: 'fixture' },
							{ id: 'hail', confidence: 60, reason: 'fallback' },
						],
					}),
				});
				return;
			}

			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify(buildPreviewResponse({ parser_used: body.parser_hint })),
			});
		});

		await page.goto(srcGitUrl());
		await expect(page.getByText('Parser selection required')).toBeVisible({ timeout: 10000 });
		await page.getByRole('button', { name: /codex/i }).click();
		await expect(page.getByRole('heading', { name: 'Source Fixture' })).toBeVisible({ timeout: 10000 });

		const url = new URL(page.url());
		expect(url.searchParams.get('parser_hint')).toBe('codex');
	});

	test('shows unsupported deployment state when parse preview is disabled', async ({ page }) => {
		await page.route('**/api/capabilities', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					auth_enabled: false,
					parse_preview_enabled: false,
					register_targets: ['local', 'git'],
					share_modes: ['web', 'git', 'json'],
				}),
			});
		});

		await page.goto(srcGitUrl());
		await expect(page.getByText('does not support source parse preview')).toBeVisible({ timeout: 10000 });
	});

	test('/git, /gh, and /resolve legacy routes are removed', async ({ page }) => {
		const gitResp = await page.request.get('/git?remote=x&ref=y&path=z');
		expect(gitResp.status()).toBe(404);

		const ghResp = await page.request.get('/gh/hwisu/opensession/main/sessions/demo.hail.jsonl');
		expect(ghResp.status()).toBe(404);

		const resolveResp = await page.request.get('/resolve/Zm9v');
		expect(resolveResp.status()).toBe(404);
	});
});
