import { type Page, type APIRequestContext } from '@playwright/test';

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000';
const PROFILE_ENV = (process.env.E2E_APP_PROFILE || process.env.VITE_APP_PROFILE || 'server')
	.trim()
	.toLowerCase();

export type AppProfile = 'server' | 'worker';
export const appProfile: AppProfile = PROFILE_ENV === 'worker' ? 'worker' : 'server';
export const isServerProfile = appProfile === 'server';
export const isWorkerProfile = appProfile === 'worker';

export interface TestUser {
	user_id: string;
	nickname: string;
	access_token: string;
	refresh_token: string;
	api_key: string;
}

let _admin: TestUser | null = null;

/**
 * Register the admin user (first registered user = admin on self-hosted).
 * Cached across tests within a worker.
 */
export async function getAdmin(request: APIRequestContext): Promise<TestUser> {
	if (_admin) return _admin;

	const email = 'pw-admin@e2e.local';
	const nickname = 'pw-admin';
	const password = 'testpass99';

	// Try registering first
	const regResp = await request.post(`${BASE_URL}/api/auth/register`, {
		data: { email, password, nickname },
	});

	let tokens: { user_id: string; nickname: string; access_token: string; refresh_token: string };

	if (regResp.ok()) {
		tokens = await regResp.json();
	} else {
		// Already registered, login instead
		const loginResp = await request.post(`${BASE_URL}/api/auth/login`, {
			data: { email, password },
		});
		if (!loginResp.ok()) {
			throw new Error(`Admin login failed: ${loginResp.status()} ${await loginResp.text()}`);
		}
		tokens = await loginResp.json();
	}

	const meResp = await request.get(`${BASE_URL}/api/auth/me`, {
		headers: { Authorization: `Bearer ${tokens.access_token}` },
	});
	const me: { api_key: string } = await meResp.json();

	_admin = { ...tokens, ...me };
	return _admin;
}

/**
 * Register a fresh non-admin user.
 */
export async function registerUser(request: APIRequestContext): Promise<TestUser> {
	const id = crypto.randomUUID().slice(0, 8);
	const email = `pw-test-${id}@e2e.local`;
	const nickname = `pw-${id}`;
	const password = 'testpass99';

	const regResp = await request.post(`${BASE_URL}/api/auth/register`, {
		data: { email, password, nickname },
	});
	if (!regResp.ok()) {
		throw new Error(`Register failed: ${regResp.status()} ${await regResp.text()}`);
	}
	const tokens: { user_id: string; nickname: string; access_token: string; refresh_token: string } =
		await regResp.json();

	const meResp = await request.get(`${BASE_URL}/api/auth/me`, {
		headers: { Authorization: `Bearer ${tokens.access_token}` },
	});
	const me: { api_key: string } = await meResp.json();

	return { ...tokens, ...me };
}

/**
 * Inject auth tokens into localStorage so the SPA treats the user as authenticated.
 */
export async function injectAuth(page: Page, user: TestUser) {
	await page.goto('/');
	await page.evaluate(
		({ accessToken, refreshToken }) => {
			localStorage.setItem('opensession_access_token', accessToken);
			localStorage.setItem('opensession_refresh_token', refreshToken);
			localStorage.setItem(
				'opensession_token_expiry',
				String(Date.now() + 3600 * 1000),
			);
		},
		{ accessToken: user.access_token, refreshToken: user.refresh_token },
	);
}

/**
 * Upload a minimal HAIL session via the API. Returns the session_id.
 */
export async function uploadSession(
	request: APIRequestContext,
	accessToken: string,
	opts?: { title?: string; teamId?: string; events?: Array<Record<string, unknown>> },
): Promise<string> {
	const sessionId = crypto.randomUUID();
	const now = new Date().toISOString();
	const defaultEvents = [
		{
			event_id: crypto.randomUUID(),
			timestamp: now,
			event_type: { type: 'UserMessage' },
			task_id: crypto.randomUUID(),
			content: { blocks: [{ type: 'Text', text: 'Hello, write a test' }] },
			duration_ms: null,
			attributes: {},
		},
		{
			event_id: crypto.randomUUID(),
			timestamp: new Date(Date.now() + 1000).toISOString(),
			event_type: { type: 'AgentMessage' },
			task_id: null,
			content: { blocks: [{ type: 'Text', text: 'Sure, here is a test.' }] },
			duration_ms: null,
			attributes: {},
		},
	];
	const events = opts?.events ?? defaultEvents;

	const firstTs = new Date(String(events[0]?.['timestamp'] ?? now)).getTime();
	const lastTs = new Date(String(events[events.length - 1]?.['timestamp'] ?? now)).getTime();
	const durationSeconds =
		Number.isFinite(firstTs) && Number.isFinite(lastTs) && lastTs >= firstTs
			? Math.floor((lastTs - firstTs) / 1000)
			: 0;
	const eventTypes = events.map((event) => {
		const maybeType = (event['event_type'] as { type?: unknown } | undefined)?.type;
		return typeof maybeType === 'string' ? maybeType : '';
	});
	const userMessageCount = eventTypes.filter((type) => type === 'UserMessage').length;
	const messageCount = eventTypes.filter((type) =>
		['UserMessage', 'AgentMessage', 'SystemMessage'].includes(type),
	).length;
	const taskCount = new Set(
		events
			.map((event) => event['task_id'])
			.filter((taskId): taskId is string => typeof taskId === 'string' && taskId.length > 0),
	).size;

	const session = {
		version: 'hail-1.0.0',
		session_id: sessionId,
		agent: {
			provider: 'anthropic',
			model: 'claude-opus-4-6',
			tool: 'claude-code',
			tool_version: '1.0.0',
		},
		context: {
			title: opts?.title || `PW Test Session ${sessionId.slice(0, 8)}`,
			description: 'Playwright test session',
			tags: ['e2e', 'playwright'],
			created_at: now,
			updated_at: now,
			related_session_ids: [],
			attributes: {},
		},
		events,
		stats: {
			event_count: events.length,
			message_count: messageCount,
			tool_call_count: 0,
			task_count: taskCount,
			duration_seconds: durationSeconds,
			total_input_tokens: 100,
			total_output_tokens: 50,
			user_message_count: userMessageCount,
			files_changed: 0,
			lines_added: 0,
			lines_removed: 0,
		},
	};

	const uploadBody: { session: typeof session; team_id?: string } = { session };
	if (opts?.teamId) {
		uploadBody.team_id = opts.teamId;
	}

	let resp = await request.post(`${BASE_URL}/api/sessions`, {
		data: uploadBody,
		headers: { Authorization: `Bearer ${accessToken}` },
	});

	// Some deployments require explicit team membership even for uploads.
	// If personal upload is rejected, create a team and retry once.
	if (resp.status() === 403 && !opts?.teamId) {
		const teamId = await createTeam(request, accessToken);
		resp = await request.post(`${BASE_URL}/api/sessions`, {
			data: { ...uploadBody, team_id: teamId },
			headers: { Authorization: `Bearer ${accessToken}` },
		});
	}

	if (!resp.ok()) {
		throw new Error(`Upload failed: ${resp.status()} ${await resp.text()}`);
	}

	const result: { id: string; url: string } = await resp.json();
	return result.id;
}

/**
 * Create a team via the API. Returns the team id.
 */
export async function createTeam(
	request: APIRequestContext,
	accessToken: string,
	name?: string,
): Promise<string> {
	const teamName = name || `pw-team-${crypto.randomUUID().slice(0, 8)}`;
	const resp = await request.post(`${BASE_URL}/api/teams`, {
		data: { name: teamName, description: 'Playwright test team', is_public: false },
		headers: { Authorization: `Bearer ${accessToken}` },
	});
	if (!resp.ok()) {
		throw new Error(`Create team failed: ${resp.status()} ${await resp.text()}`);
	}
	const team: { id: string } = await resp.json();
	return team.id;
}

/**
 * Add a member to a team by nickname. Requires team admin token.
 */
export async function addMember(
	request: APIRequestContext,
	adminToken: string,
	teamId: string,
	nickname: string,
): Promise<void> {
	const resp = await request.post(`${BASE_URL}/api/teams/${teamId}/members`, {
		data: { nickname },
		headers: { Authorization: `Bearer ${adminToken}` },
	});
	if (!resp.ok()) {
		throw new Error(`Add member failed: ${resp.status()} ${await resp.text()}`);
	}
}
