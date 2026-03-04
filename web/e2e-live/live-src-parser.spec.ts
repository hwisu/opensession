import { expect, test } from '@playwright/test';

function base64UrlEncode(value: string): string {
	return Buffer.from(value, 'utf8')
		.toString('base64')
		.replace(/\+/g, '-')
		.replace(/\//g, '_')
		.replace(/=+$/g, '');
}

function buildSuccessPreview() {
	const now = new Date().toISOString();
	return {
		parser_used: 'hail',
		parser_candidates: [{ id: 'hail', confidence: 96, reason: 'parser hint selected' }],
		session: {
			version: 'hail-1.0.0',
			session_id: 'live-src-retry',
			agent: { provider: 'openai', model: 'gpt-5', tool: 'codex' },
			context: {
				title: 'Live Parser Retry',
				description: 'live parser selection fallback test',
				tags: ['e2e', 'live'],
				created_at: now,
				updated_at: now,
				related_session_ids: [],
				attributes: {},
			},
			events: [
				{
					event_id: 'evt-1',
					timestamp: now,
					event_type: { type: 'UserMessage' },
					content: { blocks: [{ type: 'Text', text: 'parser retry event' }] },
					attributes: {},
				},
			],
			stats: {
				event_count: 1,
				message_count: 1,
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
	};
}

test.describe('Live Source Route', () => {
	// @covers web.live.src.git.parser_selection_retry
	test('parser selection route retries with explicit parser hint', async ({ page }) => {
		let parseAttempts = 0;
		await page.route('**/api/parse/preview', async (route) => {
			parseAttempts += 1;
			const body = route.request().postDataJSON() as { parser_hint?: string | null };
			if (!body?.parser_hint) {
				await route.fulfill({
					status: 422,
					contentType: 'application/json',
					body: JSON.stringify({
						code: 'parser_selection_required',
						message: 'Select a parser and retry.',
						parser_candidates: [
							{ id: 'hail', confidence: 90, reason: 'jsonl extension' },
							{ id: 'codex', confidence: 62, reason: 'jsonl extension' },
						],
					}),
				});
				return;
			}
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify(buildSuccessPreview()),
			});
		});

		const remote = base64UrlEncode('https://github.com/hwisu/opensession');
		await page.goto(`/src/git/${remote}/ref/main/path/sessions/demo.hail.jsonl`);

		await expect(page.getByRole('heading', { name: 'Parser selection required' })).toBeVisible();
		await page.getByRole('button', { name: /hail/i }).first().click();
		await expect(page).toHaveURL(/parser_hint=hail/);
		await expect(page.getByRole('heading', { name: 'Live Parser Retry' })).toBeVisible();
		expect(parseAttempts).toBeGreaterThanOrEqual(2);
	});
});
