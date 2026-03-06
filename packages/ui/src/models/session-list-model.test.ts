import assert from 'node:assert/strict';
import test from 'node:test';
import {
	createSessionListModel,
	createSessionListModelState,
	type SessionListCachePort,
} from './session-list-model.ts';

function createMemoryCache(initial: Record<string, unknown> = {}): SessionListCachePort {
	const storage = new Map(Object.entries(initial));
	return {
		read(key) {
			return (storage.get(key) as any) ?? null;
		},
		write(entry) {
			storage.set(entry.query, entry);
		},
		clear() {
			storage.clear();
		},
	};
}

test('session list model hydrates query state on first load', async () => {
	const state = createSessionListModelState();
	const model = createSessionListModel(state, {
		listSessions: async () => ({
			total: 1,
			page: 1,
			per_page: 20,
			sessions: [
				{
					id: 'session-1',
					user_id: null,
					nickname: null,
					tool: 'codex',
					agent_provider: null,
					agent_model: null,
					title: null,
					description: null,
					tags: null,
					created_at: '2026-03-05T00:00:00Z',
					uploaded_at: '2026-03-05T00:00:00Z',
					message_count: 0,
					task_count: 0,
					event_count: 0,
					duration_seconds: 0,
					total_input_tokens: 0,
					total_output_tokens: 0,
					git_repo_name: 'acme/api',
					has_errors: false,
					max_active_agents: 0,
					session_score: 0,
					score_plugin: 'default',
				},
			],
		}),
		listSessionRepos: async () => ({ repos: ['acme/web', 'acme/api'] }),
		cache: createMemoryCache(),
		getLocationSearch: () => '?search=fix&tool=codex&git_repo_name=acme/api&time_range=7d',
		validToolValues: ['', 'codex'],
		validTimeRanges: new Set(['all', '24h', '7d', '30d']),
	});

	await model.loadInitial();

	assert.equal(state.searchQuery, 'fix');
	assert.equal(state.toolFilter, 'codex');
	assert.equal(state.repoFilter, 'acme/api');
	assert.equal(state.timeRange, '7d');
	assert.equal(state.sessions.length, 1);
	assert.deepEqual(state.knownRepos, ['acme/api', 'acme/web']);
});

test('session list model keeps the newest reset request result', async () => {
	let resolveFirst: ((value: any) => void) | null = null;
	const first = new Promise((resolve) => {
		resolveFirst = resolve;
	});

	let callCount = 0;
	const state = createSessionListModelState();
	const model = createSessionListModel(state, {
		listSessions: async (params) => {
			callCount += 1;
			if (callCount === 1) {
				return first as any;
			}
			return {
				total: 1,
				page: params?.page ?? 1,
				per_page: params?.per_page ?? 20,
				sessions: [
					{
						id: 'newest',
						user_id: null,
						nickname: null,
						tool: 'codex',
						agent_provider: null,
						agent_model: null,
						title: null,
						description: null,
						tags: null,
						created_at: '2026-03-05T00:00:00Z',
						uploaded_at: '2026-03-05T00:00:00Z',
						message_count: 0,
						task_count: 0,
						event_count: 0,
						duration_seconds: 0,
						total_input_tokens: 0,
						total_output_tokens: 0,
						git_repo_name: null,
						has_errors: false,
						max_active_agents: 0,
						session_score: 0,
						score_plugin: 'default',
					},
				],
			};
		},
		listSessionRepos: async () => ({ repos: [] }),
		cache: createMemoryCache(),
		getLocationSearch: () => '',
		validToolValues: ['', 'codex'],
		validTimeRanges: new Set(['all', '24h', '7d', '30d']),
	});

	void model.fetchSessions(true);
	state.searchQuery = 'retry';
	await model.fetchSessions(true);
	resolveFirst?.({
		total: 1,
		page: 1,
		per_page: 20,
		sessions: [
			{
				id: 'stale',
				user_id: null,
				nickname: null,
				tool: 'codex',
				agent_provider: null,
				agent_model: null,
				title: null,
				description: null,
				tags: null,
				created_at: '2026-03-05T00:00:00Z',
				uploaded_at: '2026-03-05T00:00:00Z',
				message_count: 0,
				task_count: 0,
				event_count: 0,
				duration_seconds: 0,
				total_input_tokens: 0,
				total_output_tokens: 0,
				git_repo_name: null,
				has_errors: false,
				max_active_agents: 0,
				session_score: 0,
				score_plugin: 'default',
			},
		],
	});
	await first;

	assert.equal(state.sessions[0]?.id, 'newest');
});
