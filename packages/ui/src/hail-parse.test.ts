import assert from 'node:assert/strict';
import test from 'node:test';
import { parseHailInput, parseHailJsonl } from './hail-parse';

test('parseHailInput backfills extended stats fields from legacy JSON payload', () => {
	const legacy = {
		version: 'hail-1.0.0',
		session_id: 's-legacy',
		agent: {
			provider: 'openai',
			model: 'gpt-5',
			tool: 'codex',
		},
		context: {
			tags: [],
			created_at: '2026-03-03T00:00:00Z',
			updated_at: '2026-03-03T00:00:00Z',
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
		},
	};

	const parsed = parseHailInput(JSON.stringify(legacy));

	assert.equal(parsed.stats.user_message_count, 0);
	assert.equal(parsed.stats.files_changed, 0);
	assert.equal(parsed.stats.lines_added, 0);
	assert.equal(parsed.stats.lines_removed, 0);
	assert.deepEqual(parsed.context.related_session_ids, []);
	assert.deepEqual(parsed.context.attributes, {});
});

test('parseHailJsonl preserves existing extended stats values', () => {
	const text = [
		JSON.stringify({
			type: 'header',
			version: 'hail-1.0.0',
			session_id: 's-jsonl',
			agent: {
				provider: 'anthropic',
				model: 'claude-opus-4-5-20251101',
				tool: 'claude-code',
			},
			context: {
				tags: ['demo'],
				created_at: '2026-03-03T00:00:00Z',
				updated_at: '2026-03-03T00:01:00Z',
				related_session_ids: ['parent-1'],
				attributes: { source: 'test' },
			},
		}),
		JSON.stringify({
			type: 'stats',
			event_count: 10,
			message_count: 4,
			tool_call_count: 2,
			task_count: 1,
			duration_seconds: 30,
			total_input_tokens: 100,
			total_output_tokens: 20,
			user_message_count: 2,
			files_changed: 3,
			lines_added: 50,
			lines_removed: 10,
		}),
	].join('\n');

	const parsed = parseHailJsonl(text);

	assert.equal(parsed.stats.user_message_count, 2);
	assert.equal(parsed.stats.files_changed, 3);
	assert.equal(parsed.stats.lines_added, 50);
	assert.equal(parsed.stats.lines_removed, 10);
	assert.deepEqual(parsed.context.related_session_ids, ['parent-1']);
	assert.deepEqual(parsed.context.attributes, { source: 'test' });
});
