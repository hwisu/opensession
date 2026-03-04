import { expect, test } from '@playwright/test';

function buildLocalReviewBundle() {
	const now = new Date().toISOString();
	return {
		review_id: 'live-local-review-e2e-1',
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
				title: 'feat: review local route live test',
				author_name: 'Alice',
				author_email: 'alice@example.com',
				authored_at: now,
				session_ids: ['live-session-1'],
			},
		],
		sessions: [
			{
				session_id: 'live-session-1',
				ledger_ref: 'refs/remotes/origin/opensession/branches/bGl2ZS1yZXZpZXc',
				hail_path: 'v1/sr/live-session-1.hail.jsonl',
				commit_shas: ['a'.repeat(40)],
				session: {
					version: 'hail-1.0.0',
					session_id: 'live-session-1',
					agent: { provider: 'openai', model: 'gpt-5', tool: 'codex' },
					context: {
						title: 'Live Review Session',
						description: 'review local live route',
						tags: ['review', 'live'],
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
							content: { blocks: [{ type: 'Text', text: 'please review this commit' }] },
							attributes: {},
						},
					],
					stats: {
						event_count: 1,
						message_count: 1,
						tool_call_count: 0,
						task_count: 0,
						duration_seconds: 1,
						total_input_tokens: 10,
						total_output_tokens: 10,
						user_message_count: 1,
						files_changed: 0,
						lines_added: 0,
						lines_removed: 0,
					},
				},
			},
		],
	};
}

test.describe('Live Local Review Route', () => {
	// @covers web.live.review.local.render
	test('renders review bundle data on /review/local/:id', async ({ page }) => {
		await page.route('**/api/review/local/*', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify(buildLocalReviewBundle()),
			});
		});

		await page.goto('/review/local/live-local-review-e2e-1');

		await expect(page.getByText('PR #7 acme/private-repo')).toBeVisible({ timeout: 10000 });
		await expect(page.getByText('feat: review local route live test')).toBeVisible();
		await expect(page.getByRole('heading', { name: 'Live Review Session' })).toBeVisible();
	});
});
