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

test('parseHailJsonl reads ACP semantic JSONL and restores job metadata', () => {
	const text = [
		JSON.stringify({
			type: 'session.new',
			sessionId: 's-acp',
			cwd: '/tmp/repo',
			_meta: {
				opensession: {
					agent: {
						provider: 'openai',
						model: 'gpt-5',
						tool: 'codex',
					},
					context: {
						title: 'ACP session',
						description: 'semantic jsonl',
						tags: ['demo'],
						createdAt: '2026-03-03T00:00:00Z',
						updatedAt: '2026-03-03T00:01:00Z',
						relatedSessionIds: ['parent-2'],
						attributes: { source: 'acp' },
					},
					review: {
						id: 'todo-review-1',
						qa: { count: 2 },
					},
					handoff: {
						artifact_uri: 'os://artifact/handoff/123',
					},
					job: {
						protocol: 'agent_client_protocol',
						system: 'symphony',
						jobId: 'AUTH-123',
						jobTitle: 'Fix auth',
						runId: 'run-42',
						attempt: 2,
						stage: 'review',
						reviewKind: 'todo',
						status: 'pending',
						artifacts: [],
					},
					source: {
						sessionVersion: 'acp-semantic-1.0.0',
					},
				},
			},
		}),
		JSON.stringify({
			type: 'session.update',
			sessionId: 's-acp',
			update: {
				sessionUpdate: 'user_message_chunk',
				content: { type: 'text', text: 'hello from acp' },
			},
			_meta: {
				opensession: {
					diff: {
						path: 'src/lib.rs',
						unified: '@@ -1 +1 @@\n-old\n+new',
						language: 'rust',
					},
					event: {
						eventId: 'e1',
						timestamp: '2026-03-03T00:00:10Z',
						originalEventType: { type: 'UserMessage' },
						originalContent: { blocks: [{ type: 'Text', text: 'hello from acp' }] },
						attributes: { source: 'fixture' },
					},
				},
			},
		}),
		JSON.stringify({
			type: 'session.end',
			sessionId: 's-acp',
			_meta: {
				opensession: {
					stats: {
						event_count: 1,
						message_count: 1,
						tool_call_count: 0,
						task_count: 0,
						duration_seconds: 10,
						total_input_tokens: 11,
						total_output_tokens: 7,
						user_message_count: 1,
						files_changed: 0,
						lines_added: 0,
						lines_removed: 0,
					},
				},
			},
		}),
	].join('\n');

	const parsed = parseHailJsonl(text);

	assert.equal(parsed.version, 'acp-semantic-1.0.0');
	assert.equal(parsed.context.title, 'ACP session');
	assert.deepEqual(parsed.context.related_session_ids, ['parent-2']);
	assert.equal(parsed.context.attributes?.cwd, '/tmp/repo');
	assert.equal(parsed.context.attributes?.source, 'acp');
	assert.equal(parsed.context.attributes?.['opensession.job.id'], 'AUTH-123');
	assert.equal(parsed.context.attributes?.['opensession.review.id'], 'todo-review-1');
	assert.equal(
		parsed.context.attributes?.['opensession.handoff.artifact_uri'],
		'os://artifact/handoff/123',
	);
	assert.equal(parsed.stats.user_message_count, 1);
	assert.equal(parsed.events.length, 1);
	assert.equal(parsed.events[0]?.event_type.type, 'UserMessage');
	assert.equal(parsed.events[0]?.attributes?.['opensession.diff.language'], 'rust');
	assert.equal(
		parsed.events[0]?.attributes?.['opensession.diff.unified'],
		'@@ -1 +1 @@\n-old\n+new',
	);
});
