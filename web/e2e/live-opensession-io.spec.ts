import { test, expect, type APIRequestContext, type Page } from '@playwright/test';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';

const BASE_URL = process.env.BASE_URL || 'https://opensession.io';

type AuthResponse = {
	user_id: string;
	nickname: string;
	access_token: string;
	refresh_token: string;
	expires_in: number;
};

type TeamListResponse = {
	teams: Array<{
		id: string;
		name: string;
	}>;
};

type UploadResponse = {
	id: string;
	url: string;
};

function iso(offsetMs: number): string {
	return new Date(Date.now() + offsetMs).toISOString();
}

function mkEvent(
	type: string,
	offsetMs: number,
	taskId: string | null,
	text: string,
	data?: Record<string, unknown>,
) {
	return {
		event_id: crypto.randomUUID(),
		timestamp: iso(offsetMs),
		event_type: data ? { type, data } : { type },
		task_id: taskId,
		content: {
			blocks: text ? [{ type: 'Text', text }] : [],
		},
		duration_ms: null,
		attributes: {},
	};
}

function buildMultiAgentSession(opts: {
	tool: 'codex' | 'claude-code';
	provider: string;
	model: string;
	title: string;
	description: string;
	loremSeed: string;
}) {
	const sessionId = crypto.randomUUID();
	const taskMain = `task-main-${crypto.randomUUID()}`;
	const taskWorker = `task-worker-${crypto.randomUUID()}`;

	const lorem1 = `${opts.loremSeed} lorem ipsum dolor sit amet, consectetur adipiscing elit.`;
	const lorem2 =
		'Vestibulum ante ipsum primis in faucibus orci luctus et ultrices posuere cubilia curae.';
	const lorem3 = 'Suspendisse potenti. Curabitur vehicula, lorem in bibendum tincidunt, dui massa.';

	const events = [
		mkEvent('UserMessage', 0, null, `${lorem1} Please coordinate a multi-agent implementation.`),
		mkEvent('TaskStart', 500, taskMain, '', { title: 'Main agent planning' }),
		mkEvent('AgentMessage', 1000, taskMain, `${lorem2} Main agent is planning.`),
		mkEvent('TaskStart', 1500, taskWorker, '', { title: 'Worker agent execution' }),
		mkEvent('ToolCall', 2000, taskWorker, 'Reading repository files.', { name: 'Read' }),
		mkEvent(
			'ToolResult',
			2500,
			taskWorker,
			'Read completed with no errors.',
			{ name: 'Read', is_error: false },
		),
		mkEvent('AgentMessage', 3000, taskWorker, `${lorem3} Worker agent completed subtasks.`),
		mkEvent('TaskEnd', 3500, taskWorker, '', { summary: 'Worker done' }),
		mkEvent('AgentMessage', 4000, taskMain, `${lorem1} Main agent integrated worker output.`),
		mkEvent('TaskEnd', 4500, taskMain, '', { summary: 'Main done' }),
	];

	return {
		version: 'hail-1.0.0',
		session_id: sessionId,
		agent: {
			provider: opts.provider,
			model: opts.model,
			tool: opts.tool,
			tool_version: '1.0.0',
		},
		context: {
			title: opts.title,
			description: opts.description,
			tags: ['playwright', 'live-test', 'lorem-ipsum'],
			created_at: iso(0),
			updated_at: iso(4500),
			related_session_ids: [],
			attributes: {},
		},
		events,
		stats: {
			event_count: events.length,
			message_count: 4,
			tool_call_count: 2,
			task_count: 2,
			duration_seconds: 5,
			total_input_tokens: 200,
			total_output_tokens: 160,
			user_message_count: 1,
			files_changed: 0,
			lines_added: 0,
			lines_removed: 0,
		},
	};
}

async function registerUser(
	request: APIRequestContext,
	email: string,
	password: string,
	nickname: string,
): Promise<AuthResponse> {
	const resp = await request.post(`${BASE_URL}/api/auth/register`, {
		data: { email, password, nickname },
	});
	expect(resp.ok(), `register failed: ${resp.status()} ${await resp.text()}`).toBeTruthy();
	return (await resp.json()) as AuthResponse;
}

async function loginApi(
	request: APIRequestContext,
	email: string,
	password: string,
): Promise<AuthResponse> {
	const resp = await request.post(`${BASE_URL}/api/auth/login`, {
		data: { email, password },
	});
	expect(resp.ok(), `login failed: ${resp.status()} ${await resp.text()}`).toBeTruthy();
	return (await resp.json()) as AuthResponse;
}

async function listTeamsApi(request: APIRequestContext, accessToken: string): Promise<TeamListResponse> {
	const resp = await request.get(`${BASE_URL}/api/teams`, {
		headers: { Authorization: `Bearer ${accessToken}` },
	});
	expect(resp.ok(), `list teams failed: ${resp.status()} ${await resp.text()}`).toBeTruthy();
	return (await resp.json()) as TeamListResponse;
}

async function inviteByEmailApi(
	request: APIRequestContext,
	accessToken: string,
	teamId: string,
	email: string,
) {
	const resp = await request.post(`${BASE_URL}/api/teams/${teamId}/invite`, {
		headers: { Authorization: `Bearer ${accessToken}` },
		data: { email, role: 'member' },
	});
	expect(resp.ok(), `invite failed: ${resp.status()} ${await resp.text()}`).toBeTruthy();
}

async function loginViaUi(page: Page, email: string, password: string) {
	await page.goto('/login');
	await page.fill('#login-email', email);
	await page.fill('#login-password', password);
	await page.getByRole('button', { name: 'Sign In' }).click();
	await page.waitForURL((url) => !url.pathname.startsWith('/login'));
}

async function logoutViaUi(page: Page) {
	await page.goto('/settings');
	await page.getByRole('button', { name: 'Logout' }).click();
	await expect(page.getByRole('button', { name: 'Sign In' })).toBeVisible();
}

async function uploadViaFile(page: Page, teamId: string, filePath: string): Promise<string> {
	await page.goto('/upload');
	await page.fill('#team-select', teamId);
	await page.setInputFiles('#file-input', filePath);
	await page.getByRole('button', { name: 'Upload Session' }).click();
	await page.waitForURL(/\/session\/[^/]+$/);
	const url = new URL(page.url());
	return url.pathname.split('/').pop() || '';
}

test('live opensession.io auth-team-inbox-upload flow', async ({ page, request }) => {
	test.setTimeout(8 * 60 * 1000);
	page.setDefaultTimeout(20_000);

	const runTag = `${Date.now()}-${crypto.randomBytes(3).toString('hex')}`;
	const user1 = {
		email: `pw.live.owner.${runTag}@example.com`,
		nickname: `pw-owner-${runTag.slice(-6)}`,
		password: 'Pwtest901',
		newPassword: 'Pwtest902',
	};
	const user2 = {
		email: `pw.live.member.${runTag}@example.com`,
		nickname: `pw-member-${runTag.slice(-6)}`,
		password: 'Pwtest903',
	};
	const teamName = `pw-live-team-${runTag.slice(-8)}`;
	const publicTitle = `PW Public Codex ${runTag}`;
	const teamTitle = `PW Team ClaudeCode ${runTag}`;

	let ownerAuth: AuthResponse | null = null;
	let memberAuth: AuthResponse | null = null;
	let ownerReauth: AuthResponse | null = null;
	let teamId = '';
	let personalTeamId = 'personal';
	let publicSessionId = '';
	let teamSessionId = '';
	let tempDir = '';

	const cleanupLog: string[] = [];

	try {
		console.log(`[live-test] runTag=${runTag}`);
		console.log('[live-test] register user1/user2');
		ownerAuth = await registerUser(request, user1.email, user1.password, user1.nickname);
		memberAuth = await registerUser(request, user2.email, user2.password, user2.nickname);

		console.log('[live-test] user1 login + password change');
		await loginViaUi(page, user1.email, user1.password);

		await page.goto('/settings');
		await page.fill('#current-pw', user1.password);
		await page.fill('#new-pw', user1.newPassword);
		await page.fill('#confirm-pw', user1.newPassword);
		await page.getByRole('button', { name: 'Change Password' }).click();
		await expect(page.getByText('Password changed successfully')).toBeVisible();
		await logoutViaUi(page);

		console.log('[live-test] user1 relogin + create team + invite user2');
		await loginViaUi(page, user1.email, user1.newPassword);

		await page.goto('/teams');
		await page.getByRole('button', { name: '[Create Team]' }).click();
		await page.fill('#team-name', teamName);
		await page.fill('#team-desc', `Playwright live test ${runTag}`);
		await page.getByRole('button', { name: 'Create' }).click();
		await page.getByRole('link', { name: teamName }).click();
		await page.waitForURL(/\/teams\/[^/]+$/);
		teamId = page.url().split('/teams/')[1]?.split('?')[0] || '';
		expect(teamId, 'team id should be captured from URL').not.toBe('');
		console.log(`[live-test] created teamId=${teamId}`);

		ownerReauth = await loginApi(request, user1.email, user1.newPassword);
		await inviteByEmailApi(request, ownerReauth.access_token, teamId, user2.email);
		console.log('[live-test] invited user2 by email via API');
		await logoutViaUi(page);

		console.log('[live-test] user2 login + check inbox + accept invitation');
		await loginViaUi(page, user2.email, user2.password);
		await page.goto('/invitations');
		await expect(page.getByText(teamName)).toBeVisible();
		const inviteCard = page.locator('div', { hasText: teamName }).first();
		await inviteCard.getByRole('button', { name: 'Accept' }).click();
		await page.waitForURL(new RegExp(`/teams/${teamId}$`));

		console.log('[live-test] resolve personal team + create temp upload files');
		const teams = await listTeamsApi(request, memberAuth.access_token);
		const personal = teams.teams.find((t) => t.id === 'personal');
		if (personal) {
			personalTeamId = personal.id;
		}

		tempDir = await fs.mkdtemp(path.join(os.tmpdir(), `opensession-live-${runTag}-`));
		const publicFile = path.join(tempDir, 'public-codex.json');
		const teamFile = path.join(tempDir, 'team-claude-code.json');

		// Start with temporary empty files, then write synthetic lorem-ipsum sessions.
		await fs.writeFile(publicFile, '');
		await fs.writeFile(teamFile, '');
		await fs.writeFile(
			publicFile,
			JSON.stringify(
				buildMultiAgentSession({
					tool: 'codex',
					provider: 'openai',
					model: 'gpt-5-codex',
					title: publicTitle,
					description: 'Lorem ipsum public multi-agent session',
					loremSeed: 'Public Codex session',
				}),
				null,
				2,
			),
		);
		await fs.writeFile(
			teamFile,
			JSON.stringify(
				buildMultiAgentSession({
					tool: 'claude-code',
					provider: 'anthropic',
					model: 'claude-opus-4-6',
					title: teamTitle,
					description: 'Lorem ipsum team multi-agent session',
					loremSeed: 'Team ClaudeCode session',
				}),
				null,
				2,
			),
		);

		console.log('[live-test] upload public codex session');
		publicSessionId = await uploadViaFile(page, personalTeamId, publicFile);
		expect(publicSessionId).not.toBe('');
		console.log(`[live-test] publicSessionId=${publicSessionId}`);

		console.log('[live-test] upload team claude-code session');
		teamSessionId = await uploadViaFile(page, teamId, teamFile);
		expect(teamSessionId).not.toBe('');
		console.log(`[live-test] teamSessionId=${teamSessionId}`);

		console.log('[live-test] verify team session visibility');
		await page.goto(`/teams/${teamId}`);
		await expect(page.getByText(teamTitle)).toBeVisible();

		console.log('[live-test] logout and verify public session visibility');
		await logoutViaUi(page);

		await page.goto(`/session/${publicSessionId}`);
		await expect(page.getByRole('heading', { name: publicTitle })).toBeVisible();
		console.log('[live-test] scenario completed');
	} finally {
		if (memberAuth && publicSessionId) {
			const del = await request.delete(`${BASE_URL}/api/sessions/${publicSessionId}`, {
				headers: { Authorization: `Bearer ${memberAuth.access_token}` },
			});
			cleanupLog.push(`delete public session: ${del.status()}`);
		}
		if (memberAuth && teamSessionId) {
			const del = await request.delete(`${BASE_URL}/api/sessions/${teamSessionId}`, {
				headers: { Authorization: `Bearer ${memberAuth.access_token}` },
			});
			cleanupLog.push(`delete team session: ${del.status()}`);
		}

		if (teamId && ownerAuth && memberAuth) {
			try {
				const ownerForCleanup =
					ownerReauth ?? (await loginApi(request, user1.email, user1.newPassword));
				const rm = await request.delete(
					`${BASE_URL}/api/teams/${teamId}/members/${memberAuth.user_id}`,
					{
						headers: { Authorization: `Bearer ${ownerForCleanup.access_token}` },
					},
				);
				cleanupLog.push(`remove team member: ${rm.status()}`);
			} catch (error) {
				cleanupLog.push(`remove team member failed: ${String(error)}`);
			}
		}

		if (ownerReauth) {
			const logoutResp = await request.post(`${BASE_URL}/api/auth/logout`, {
				data: { refresh_token: ownerReauth.refresh_token },
			});
			cleanupLog.push(`owner logout: ${logoutResp.status()}`);
		}
		if (memberAuth) {
			const logoutResp = await request.post(`${BASE_URL}/api/auth/logout`, {
				data: { refresh_token: memberAuth.refresh_token },
			});
			cleanupLog.push(`member logout: ${logoutResp.status()}`);
		}

		if (tempDir) {
			await fs.rm(tempDir, { recursive: true, force: true });
		}
		console.log(`[live-test-cleanup] ${cleanupLog.join(' | ')}`);
	}
});
