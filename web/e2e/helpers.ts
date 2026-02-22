import { type Page, type APIRequestContext } from '@playwright/test';

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000';

export interface TestUser {
	user_id: string;
	nickname: string;
	access_token: string;
	refresh_token: string;
}

export interface ApiCapabilities {
	auth_enabled: boolean;
	parse_preview_enabled: boolean;
	register_targets: string[];
	share_modes: string[];
}

export interface SessionEventSpec {
	type: 'UserMessage' | 'AgentMessage' | 'SystemMessage';
	text: string;
	task_id?: string | null;
}

export interface SessionFixture {
	id: string;
	title: string;
	summary: Record<string, unknown>;
	raw_jsonl: string;
}

let _admin: TestUser | null = null;
let _capabilities: ApiCapabilities | null = null;

const DEFAULT_CAPABILITIES: ApiCapabilities = {
	auth_enabled: false,
	parse_preview_enabled: false,
	register_targets: ['local', 'git'],
	share_modes: ['web', 'git', 'json'],
};

export function buildCapabilities(overrides?: Partial<ApiCapabilities>): ApiCapabilities {
	return {
		...DEFAULT_CAPABILITIES,
		...overrides,
		register_targets: overrides?.register_targets ?? DEFAULT_CAPABILITIES.register_targets,
		share_modes: overrides?.share_modes ?? DEFAULT_CAPABILITIES.share_modes,
	};
}

export async function mockCapabilities(page: Page, overrides?: Partial<ApiCapabilities>) {
	const capabilities = buildCapabilities(overrides);
	await page.route('**/api/capabilities', async (route) => {
		await route.fulfill({
			status: 200,
			contentType: 'application/json',
			body: JSON.stringify(capabilities),
		});
	});
}

export async function getCapabilities(request: APIRequestContext): Promise<ApiCapabilities> {
	if (_capabilities) return _capabilities;
	const resp = await request.get(`${BASE_URL}/api/capabilities`);
	if (!resp.ok()) {
		throw new Error(`Capabilities request failed: ${resp.status()} ${await resp.text()}`);
	}
	const raw = (await resp.json()) as Partial<ApiCapabilities>;
	_capabilities = {
		auth_enabled: !!raw.auth_enabled,
		parse_preview_enabled: !!raw.parse_preview_enabled,
		register_targets: Array.isArray(raw.register_targets) ? raw.register_targets : [],
		share_modes: Array.isArray(raw.share_modes) ? raw.share_modes : [],
	};
	return _capabilities;
}

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

	_admin = tokens;
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

	return tokens;
}

/**
 * Inject auth tokens into localStorage so the SPA treats the user as authenticated.
 */
export async function injectAuth(page: Page, user: TestUser) {
	await page.goto('/sessions');
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

function eventTypeFromName(type: SessionEventSpec['type']) {
	return { type };
}

function toHailJsonl(session: Record<string, unknown>, events: Array<Record<string, unknown>>) {
	const header = {
		type: 'header',
		version: session.version,
		session_id: session.session_id,
		agent: session.agent,
		context: session.context,
	};
	const eventRows = events.map((event) => ({
		type: 'event',
		...event,
	}));
	const stats = {
		type: 'stats',
		...(session.stats as Record<string, unknown>),
	};
	return [header, ...eventRows, stats].map((row) => JSON.stringify(row)).join('\n') + '\n';
}

export function createSessionFixture(opts?: {
	id?: string;
	title?: string;
	tool?: string;
	provider?: string;
	model?: string;
	events?: SessionEventSpec[];
	nickname?: string;
	user_id?: string;
}): SessionFixture {
	const sessionId = opts?.id ?? crypto.randomUUID();
	const now = new Date().toISOString();
	const title = opts?.title ?? `PW Test Session ${sessionId.slice(0, 8)}`;
	const tool = opts?.tool ?? 'claude-code';
	const provider = opts?.provider ?? 'anthropic';
	const model = opts?.model ?? 'claude-opus-4-6';
	const defaultEvents: SessionEventSpec[] = [
		{
			type: 'UserMessage',
			text: 'Hello, write a test',
			task_id: crypto.randomUUID(),
		},
		{
			type: 'AgentMessage',
			text: 'Sure, here is a test.',
			task_id: null,
		},
	];
	const sourceEvents = opts?.events ?? defaultEvents;
	const events = sourceEvents.map((event, idx) => ({
		event_id: crypto.randomUUID(),
		timestamp: new Date(Date.now() + idx * 1000).toISOString(),
		event_type: eventTypeFromName(event.type),
		task_id: event.task_id === undefined ? null : event.task_id,
		content: { blocks: [{ type: 'Text', text: event.text }] },
		duration_ms: null,
		attributes: {},
	}));

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
			provider,
			model,
			tool,
			tool_version: '1.0.0',
		},
		context: {
			title,
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

	const summary = {
		id: sessionId,
		user_id: opts?.user_id ?? 'u-e2e',
		nickname: opts?.nickname ?? 'pw-e2e',
		tool,
		agent_provider: provider,
		agent_model: model,
		title,
		description: session.context.description,
		tags: session.context.tags.join(','),
		created_at: now,
		uploaded_at: now,
		message_count: messageCount,
		task_count: taskCount,
		event_count: events.length,
		duration_seconds: durationSeconds,
		total_input_tokens: 100,
		total_output_tokens: 50,
		git_remote: null,
		git_branch: null,
		git_commit: null,
		git_repo_name: null,
		pr_number: null,
		pr_url: null,
		working_directory: null,
		files_modified: null,
		files_read: null,
		has_errors: false,
		max_active_agents: 1,
		session_score: 0,
		score_plugin: 'none',
	};

	return {
		id: sessionId,
		title,
		summary,
		raw_jsonl: toHailJsonl(session, events),
	};
}

export async function mockSessionApis(
	page: Page,
	fixture: SessionFixture,
	options?: { include_in_list?: boolean },
) {
	const includeInList = options?.include_in_list ?? true;
	const listPayload = {
		sessions: includeInList ? [fixture.summary] : [],
		total: includeInList ? 1 : 0,
		page: 1,
		per_page: 50,
	};

	await page.route('**/api/sessions', async (route) => {
		await route.fulfill({
			status: 200,
			contentType: 'application/json',
			body: JSON.stringify(listPayload),
		});
	});
	await page.route('**/api/sessions?*', async (route) => {
		await route.fulfill({
			status: 200,
			contentType: 'application/json',
			body: JSON.stringify(listPayload),
		});
	});
	await page.route(`**/api/sessions/${fixture.id}`, async (route) => {
		await route.fulfill({
			status: 200,
			contentType: 'application/json',
			body: JSON.stringify(fixture.summary),
		});
	});
	await page.route(`**/api/sessions/${fixture.id}/raw`, async (route) => {
		await route.fulfill({
			status: 200,
			contentType: 'text/plain; charset=utf-8',
			body: fixture.raw_jsonl,
		});
	});
}
