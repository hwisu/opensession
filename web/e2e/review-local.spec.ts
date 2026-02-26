import { expect, test } from '@playwright/test';

function buildReviewBundle(overrides?: Partial<Record<string, unknown>>) {
	const now = new Date().toISOString();
	return {
		review_id: 'gh-acme-private-repo-pr7-abc1234',
		generated_at: now,
		pr: {
			url: 'https://github.com/acme/private-repo/pull/7',
			owner: 'acme',
			repo: 'private-repo',
			number: 7,
			remote: 'origin',
			base_sha: '0'.repeat(40),
			head_sha: 'a'.repeat(40),
		},
		commits: [
			{
				sha: 'a'.repeat(40),
				title: 'feat: add review flow',
				author_name: 'Alice',
				author_email: 'alice@example.com',
				authored_at: now,
				session_ids: ['s-review-1'],
			},
		],
		sessions: [
			{
				session_id: 's-review-1',
				ledger_ref: 'refs/remotes/origin/opensession/branches/ZmVhdHVyZS9yZXZpZXc',
				hail_path: 'v1/sr/s-review-1.hail.jsonl',
				commit_shas: ['a'.repeat(40)],
				session: {
					version: 'hail-1.0.0',
					session_id: 's-review-1',
					agent: {
						provider: 'openai',
						model: 'gpt-5',
						tool: 'codex',
					},
					context: {
						title: 'Review Fixture Session',
						description: 'fixture',
						tags: ['review'],
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
							content: { blocks: [{ type: 'Text', text: 'please review this PR' }] },
							attributes: {},
						},
						{
							event_id: 'tool-call-1',
							timestamp: now,
							event_type: { type: 'ToolCall', data: { name: 'request_user_input' } },
							content: {
								blocks: [
									{
										type: 'Json',
										data: {
											questions: [
												{
													id: 'review_mode',
													header: 'Review Mode',
													question: 'Choose review mode',
													options: [
														{
															label: 'TUI (Recommended)',
															description: 'Terminal-first review flow',
														},
														{
															label: 'Web',
															description: 'Browser-based review flow',
														},
													],
												},
											],
										},
									},
								],
							},
							attributes: {
								'semantic.call_id': 'call-review-1',
							},
						},
						{
							event_id: 'tool-result-1',
							timestamp: now,
							event_type: {
								type: 'ToolResult',
								data: {
									name: 'request_user_input',
									is_error: false,
									call_id: 'call-review-1',
								},
							},
							content: {
								blocks: [
									{
										type: 'Text',
										text: JSON.stringify({
											answers: {
												review_mode: {
													answers: ['TUI (Recommended)'],
												},
											},
										}),
									},
								],
							},
							attributes: {
								'semantic.call_id': 'call-review-1',
							},
						},
						{
							event_id: 'a1',
							timestamp: now,
							event_type: { type: 'AgentMessage' },
							content: { blocks: [{ type: 'Text', text: 'reviewed and suggested fixes' }] },
							attributes: {},
						},
					],
					stats: {
						event_count: 4,
						message_count: 2,
						tool_call_count: 1,
						task_count: 0,
						duration_seconds: 1,
						total_input_tokens: 10,
						total_output_tokens: 20,
						user_message_count: 1,
						files_changed: 0,
						lines_added: 0,
						lines_removed: 0,
					},
				},
			},
		],
		...overrides,
	};
}

test.describe('Local Review Route', () => {
	test('renders commit group and mapped session', async ({ page }) => {
		await page.route('**/api/review/local/*', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify(buildReviewBundle()),
			});
		});

		await page.goto('/review/local/gh-acme-private-repo-pr7-abc1234');
		await expect(page.getByText('PR #7 acme/private-repo')).toBeVisible({ timeout: 10000 });
		await expect(page.getByText('feat: add review flow')).toBeVisible();
		await expect(page.getByRole('heading', { name: 'Review Fixture Session' })).toBeVisible();
		await expect(page.getByRole('tab', { name: 'Native (codex)' })).toBeVisible();
		await page.getByRole('tab', { name: 'Native (codex)' }).click();
		await expect(page.getByRole('tab', { name: 'Native (codex)' })).toHaveAttribute(
			'aria-selected',
			'true',
		);

		await page.getByRole('button', { name: /request_user_input/ }).first().click();
		await expect(page.locator('text=review_mode').first()).toBeVisible();
		await expect(page.locator('text=Choose review mode').first()).toBeVisible();
		await expect(page.locator('text=TUI (Recommended)').first()).toBeVisible();
	});

	test('query params deep-link to target commit/session', async ({ page }) => {
		const now = new Date().toISOString();
		const commitA = 'a'.repeat(40);
		const commitB = 'b'.repeat(40);
		const bundle = buildReviewBundle({
			commits: [
				{
					sha: commitA,
					title: 'feat: commit-a',
					author_name: 'Alice',
					author_email: 'alice@example.com',
					authored_at: now,
					session_ids: ['s-review-1'],
				},
				{
					sha: commitB,
					title: 'feat: commit-b',
					author_name: 'Bob',
					author_email: 'bob@example.com',
					authored_at: now,
					session_ids: ['s-review-2'],
				},
			],
			sessions: [
				{
					session_id: 's-review-1',
					ledger_ref: 'refs/remotes/origin/opensession/branches/ZmVhdHVyZS9yZXZpZXc',
					hail_path: 'v1/sr/s-review-1.hail.jsonl',
					commit_shas: [commitA],
					session: {
						version: 'hail-1.0.0',
						session_id: 's-review-1',
						agent: { provider: 'openai', model: 'gpt-5', tool: 'codex' },
						context: {
							title: 'First Session',
							description: 'first',
							tags: ['review'],
							created_at: now,
							updated_at: now,
							related_session_ids: [],
							attributes: {},
						},
						events: [],
						stats: {
							event_count: 0,
							message_count: 0,
							tool_call_count: 0,
							task_count: 0,
							duration_seconds: 0,
							total_input_tokens: 0,
							total_output_tokens: 0,
							user_message_count: 0,
							files_changed: 0,
							lines_added: 0,
							lines_removed: 0,
						},
					},
				},
				{
					session_id: 's-review-2',
					ledger_ref: 'refs/remotes/origin/opensession/branches/ZmVhdHVyZS9yZXZpZXc',
					hail_path: 'v1/sr/s-review-2.hail.jsonl',
					commit_shas: [commitB],
					session: {
						version: 'hail-1.0.0',
						session_id: 's-review-2',
						agent: { provider: 'openai', model: 'gpt-5', tool: 'codex' },
						context: {
							title: 'Second Session',
							description: 'second',
							tags: ['review'],
							created_at: now,
							updated_at: now,
							related_session_ids: [],
							attributes: {},
						},
						events: [],
						stats: {
							event_count: 0,
							message_count: 0,
							tool_call_count: 0,
							task_count: 0,
							duration_seconds: 0,
							total_input_tokens: 0,
							total_output_tokens: 0,
							user_message_count: 0,
							files_changed: 0,
							lines_added: 0,
							lines_removed: 0,
						},
					},
				},
			],
		});

		await page.route('**/api/review/local/*', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify(bundle),
			});
		});

		await page.goto(
			`/review/local/gh-acme-private-repo-pr7-abc1234?session=s-review-2&commit=${commitB}`,
		);
		await expect(page.getByText('feat: commit-b')).toBeVisible({ timeout: 10000 });
		await expect(page.getByRole('heading', { name: 'Second Session' })).toBeVisible();
	});

	test('shows error state when bundle API fails', async ({ page }) => {
		await page.route('**/api/review/local/*', async (route) => {
			await route.fulfill({
				status: 404,
				contentType: 'application/json',
				body: JSON.stringify({ code: 'not_found', message: 'local review bundle not found' }),
			});
		});

		await page.goto('/review/local/gh-acme-private-repo-pr7-missing');
		await expect(page.getByText('local review bundle not found')).toBeVisible({ timeout: 10000 });
	});
});
