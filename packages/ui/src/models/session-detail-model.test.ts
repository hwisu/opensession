import assert from 'node:assert/strict';
import test from 'node:test';
import { ApiError } from '../api-internal/errors.ts';
import {
	askSessionChangesSurface,
	loadSessionDetailState,
} from './session-detail-model.ts';

test('session detail loader treats desktop 501 runtime settings as unsupported, not fatal', async () => {
	const result = await loadSessionDetailState(
		{
			getSession: async () => ({
				version: 'hail-1.0.0',
				session_id: 'session-1',
				agent: { provider: 'openai', model: 'gpt-5', tool: 'codex' },
				context: { tags: [], created_at: '2026-03-05T00:00:00Z', updated_at: '2026-03-05T00:00:00Z' },
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
			}),
			getSessionDetail: async () => ({
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
				has_errors: false,
				max_active_agents: 0,
				session_score: 0,
				score_plugin: 'default',
				linked_sessions: [],
			}),
			getSessionSemanticSummary: async () => ({
				session_id: 'session-1',
				summary: {},
				source_kind: 'session_only',
				generation_kind: 'heuristic_fallback',
				error: null,
				diff_tree: [],
			}),
			getRuntimeSettings: async () => {
				throw new ApiError(501, '{"code":"desktop_http_api_unavailable","message":"unsupported"}');
			},
			regenerateSessionSemanticSummary: async () => {
				throw new Error('unused');
			},
			readSessionChanges: async () => {
				throw new Error('unused');
			},
			askSessionChanges: async () => {
				throw new Error('unused');
			},
		},
		'session-1',
	);

	assert.equal(result.error, null);
	assert.equal(result.changeReaderSupported, false);
	assert.equal(result.changeReaderRuntimeError, null);
});

test('session detail ask surface rejects empty question before transport call', async () => {
	const result = await askSessionChangesSurface(
		{
			getSession: async () => {
				throw new Error('unused');
			},
			getSessionDetail: async () => {
				throw new Error('unused');
			},
			getSessionSemanticSummary: async () => {
				throw new Error('unused');
			},
			getRuntimeSettings: async () => {
				throw new Error('unused');
			},
			regenerateSessionSemanticSummary: async () => {
				throw new Error('unused');
			},
			readSessionChanges: async () => {
				throw new Error('unused');
			},
			askSessionChanges: async () => {
				throw new Error('transport should not run');
			},
		},
		'session-1',
		'   ',
		'summary_only',
	);

	assert.equal(result.error, 'Ask a question first.');
});
