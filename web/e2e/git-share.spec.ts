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
			session_id: 'git-fixture-1',
			agent: {
				provider: 'openai',
				model: 'gpt-5',
				tool: 'codex',
			},
			context: {
				title: 'Git Share Fixture',
				description: 'fixture session',
				tags: ['git', 'share'],
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
				{
					event_id: 't1',
					timestamp: now,
					event_type: { type: 'ToolCall', data: { name: 'read_file' } },
					content: { blocks: [{ type: 'Text', text: 'reading' }] },
					attributes: {},
				},
			],
			stats: {
				event_count: 3,
				message_count: 2,
				tool_call_count: 1,
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

function gitUrl(): string {
	const params = new URLSearchParams({
		remote: 'https://github.com/hwisu/opensession',
		ref: 'main',
		path: 'sessions/demo.hail.jsonl',
	});
	return `/git?${params.toString()}`;
}

test.describe('Git Share Route', () => {
	test('renders session automatically from /git query URL', async ({ page }) => {
		await page.route('**/api/capabilities', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					auth_enabled: false,
					upload_enabled: true,
					ingest_preview_enabled: true,
					gh_share_enabled: true,
				}),
			});
		});
		await page.route('**/api/ingest/preview', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify(buildPreviewResponse()),
			});
		});

		await page.goto(gitUrl());
		await expect(page.getByRole('heading', { name: 'Git Share Fixture' })).toBeVisible({ timeout: 10000 });
		await expect(page.getByText('hello from user')).toBeVisible();
		await expect(page.getByText('hello from assistant')).toBeVisible();
	});

	test('switches view mode and syncs URL', async ({ page }) => {
		await page.route('**/api/capabilities', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					auth_enabled: false,
					upload_enabled: true,
					ingest_preview_enabled: true,
					gh_share_enabled: true,
				}),
			});
		});
		await page.route('**/api/ingest/preview', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify(buildPreviewResponse()),
			});
		});

		await page.goto(gitUrl());
		await expect(page.getByRole('tab', { name: 'Native (codex)' })).toBeVisible();
		await page.getByRole('tab', { name: 'Native (codex)' }).click();
		await expect(page).toHaveURL(/view=native/);
		await page.getByRole('tab', { name: 'Unified' }).click();
		await expect(page).toHaveURL(/view=unified/);
	});

	test('supports parser selection retry flow on /git', async ({ page }) => {
		await page.route('**/api/capabilities', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					auth_enabled: false,
					upload_enabled: true,
					ingest_preview_enabled: true,
					gh_share_enabled: true,
				}),
			});
		});
		await page.route('**/api/ingest/preview', async (route) => {
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

		await page.goto(gitUrl());
		await expect(page.getByText('Parser selection required')).toBeVisible({ timeout: 10000 });
		await page.getByRole('button', { name: /codex/i }).click();
		await expect(page.getByRole('heading', { name: 'Git Share Fixture' })).toBeVisible({ timeout: 10000 });

		const url = new URL(page.url());
		expect(url.searchParams.get('parser_hint')).toBe('codex');
	});

	test('shows unsupported deployment state when git share is disabled', async ({ page }) => {
		await page.route('**/api/capabilities', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					auth_enabled: false,
					upload_enabled: false,
					ingest_preview_enabled: false,
					gh_share_enabled: false,
				}),
			});
		});

		await page.goto(gitUrl());
		await expect(page.getByText('does not support git source preview')).toBeVisible({ timeout: 10000 });
	});

	test('legacy /gh route redirects to /git compatibility URL', async ({ page }) => {
		await page.route('**/api/capabilities', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					auth_enabled: false,
					upload_enabled: true,
					ingest_preview_enabled: true,
					gh_share_enabled: true,
				}),
			});
		});
		await page.route('**/api/ingest/preview', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify(buildPreviewResponse()),
			});
		});

		await page.goto('/gh/hwisu/opensession/main/sessions/demo.hail.jsonl');
		await expect(page).toHaveURL(/\/git\?/);
		await expect(page.getByRole('heading', { name: 'Git Share Fixture' })).toBeVisible({ timeout: 10000 });
	});
});
