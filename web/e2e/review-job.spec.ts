import { expect, test } from '@playwright/test';

function buildSession(sessionId: string, title: string, now: string) {
	return {
		session_id: sessionId,
		ledger_ref: `os://src/local/${sessionId}`,
		hail_path: `/tmp/${sessionId}.hail.jsonl`,
		commit_shas: [],
		session: {
			version: 'hail-1.0.0',
			session_id: sessionId,
			agent: {
				provider: 'openai',
				model: 'gpt-5',
				tool: 'codex',
			},
			context: {
				title,
				description: `${title} fixture`,
				tags: ['review'],
				created_at: now,
				updated_at: now,
				related_session_ids: [],
				attributes: {},
			},
			events: [
				{
					event_id: `${sessionId}-u1`,
					timestamp: now,
					event_type: { type: 'UserMessage' },
					content: { blocks: [{ type: 'Text', text: `review ${title}` }] },
					attributes: {},
				},
				{
					event_id: `${sessionId}-a1`,
					timestamp: now,
					event_type: { type: 'AgentMessage' },
					content: { blocks: [{ type: 'Text', text: `result for ${title}` }] },
					attributes: {},
				},
			],
			stats: {
				event_count: 2,
				message_count: 2,
				tool_call_count: 0,
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
	};
}

function buildJobReviewBundle(overrides?: Partial<Record<string, unknown>>) {
	const now = new Date().toISOString();
	return {
		job: {
			protocol: 'agent_communication_protocol',
			system: 'symphony',
			job_id: 'AUTH-123',
			job_title: 'Fix auth bug',
		},
		selected_review: {
			session_id: 's-todo',
			run_id: 'run-2',
			attempt: 2,
			kind: 'todo',
			status: 'pending',
			created_at: now,
		},
		runs: [
			{
				run_id: 'run-2',
				attempt: 2,
				status: 'pending',
				sessions: [buildSession('s-plan', 'Planning Session', now), buildSession('s-todo', 'Todo Review Session', now)],
				artifacts: [
					{
						kind: 'plan',
						label: 'Plan note',
						uri: 'file:///tmp/plan.md',
					},
				],
			},
		],
		review_digest: {
			qa: [
				{
					question: 'What should happen next?',
					answer: 'Implement the fix and prepare handoff.',
				},
			],
			modified_files: ['src/auth.ts', 'src/auth.test.ts'],
			test_files: ['src/auth.test.ts'],
		},
		semantic_summary: {
			changes: 'Refines auth flow and documents next steps.',
			auth_security: 'Touches auth checks but keeps token handling unchanged.',
			layer_file_changes: [
				{
					layer: 'auth',
					summary: 'Adjusts the auth workflow.',
					files: ['src/auth.ts'],
				},
			],
			source_kind: 'session_summary',
			generation_kind: 'local',
			provider: 'openai',
			model: 'gpt-5',
			diff_tree: [],
		},
		handoff_artifact_uri: 'file:///tmp/handoff.md',
		...overrides,
	};
}

test.describe('Job Review Route', () => {
	test('renders todo review bundle and defaults kind query to todo', async ({ page }) => {
		let requestedKind: string | null = null;
		await page.route(/\/api\/review\/job\/.*$/, async (route) => {
			const url = new URL(route.request().url());
			requestedKind = url.searchParams.get('kind');
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify(buildJobReviewBundle()),
			});
		});

		await page.goto('/review/job/AUTH-123');
		await expect(page.getByText('Fix auth bug · AUTH-123')).toBeVisible({ timeout: 10000 });
		await expect(page.getByText('todo review', { exact: true })).toBeVisible();
		await expect(page.getByText('Run History (1)')).toBeVisible();
		await expect(page.getByText('Reviewer Quick Digest')).toBeVisible();
		await expect(page.getByText('Artifacts', { exact: true })).toBeVisible();
		await expect(page.getByText('Run Sessions (2)')).toBeVisible();
		await expect(page.getByRole('heading', { name: 'Planning Session' })).toBeVisible();
		expect(requestedKind).toBe('todo');
	});

	test('query params deep-link to selected run and session without leaking session to API', async ({
		page,
	}) => {
		const now = new Date().toISOString();
		let requestedKind: string | null = null;
		let requestedRunId: string | null = null;
		let requestedSession: string | null = null;
		const bundle = buildJobReviewBundle({
			selected_review: {
				session_id: 's-done',
				run_id: 'run-2',
				attempt: 2,
				kind: 'done',
				status: 'completed',
				created_at: now,
			},
			runs: [
				{
					run_id: 'run-1',
					attempt: 1,
					status: 'failed',
					sessions: [buildSession('s-old', 'Old Review Session', now)],
					artifacts: [],
				},
				{
					run_id: 'run-2',
					attempt: 2,
					status: 'completed',
					sessions: [
						buildSession('s-todo', 'Todo Review Session', now),
						buildSession('s-done', 'Done Review Session', now),
					],
					artifacts: [
						{
							kind: 'handoff',
							label: 'Handoff notes',
							uri: 'file:///tmp/handoff.md',
						},
					],
				},
			],
		});

		await page.route(/\/api\/review\/job\/.*$/, async (route) => {
			const url = new URL(route.request().url());
			requestedKind = url.searchParams.get('kind');
			requestedRunId = url.searchParams.get('run_id');
			requestedSession = url.searchParams.get('session');
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify(bundle),
			});
		});

		await page.goto('/review/job/AUTH-123?kind=done&run_id=run-2&session=s-done');
		await expect(page.getByText('done review', { exact: true })).toBeVisible({ timeout: 10000 });
		await expect(page.getByText('selected run run-2')).toBeVisible();
		await expect(page.getByRole('heading', { name: 'Done Review Session' })).toBeVisible();
		await expect(page.getByRole('link', { name: 'file:///tmp/handoff.md' }).first()).toBeVisible();
		expect(requestedKind).toBe('done');
		expect(requestedRunId).toBe('run-2');
		expect(requestedSession).toBeNull();
	});

	test('shows error state when job review API fails', async ({ page }) => {
		await page.route(/\/api\/review\/job\/.*$/, async (route) => {
			await route.fulfill({
				status: 404,
				contentType: 'application/json',
				body: JSON.stringify({ code: 'not_found', message: 'job review bundle not found' }),
			});
		});

		await page.goto('/review/job/AUTH-404?kind=done');
		await expect(page.getByText('job review bundle not found')).toBeVisible({ timeout: 10000 });
	});
});
