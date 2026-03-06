import assert from 'node:assert/strict';
import test from 'node:test';
import {
	extractFileEditDiff,
	isBoilerplateEvent,
	pairToolCallResults,
	prepareTimelineEvents,
} from './event-helpers';
import type { Event } from './types';

function textEvent(args: {
	id: string;
	type: Event['event_type'];
	text?: string;
	timestamp?: string;
	task_id?: string;
}): Event {
	return {
		event_id: args.id,
		timestamp: args.timestamp ?? '2026-03-03T00:00:00Z',
		event_type: args.type,
		task_id: args.task_id,
		content: args.text != null ? { blocks: [{ type: 'Text', text: args.text }] } : { blocks: [] },
		duration_ms: undefined,
		attributes: {},
	};
}

test('isBoilerplateEvent treats single-dot exec output as status noise', () => {
	const dotResult = textEvent({
		id: 'e1',
		type: {
			type: 'ToolResult',
			data: { name: 'exec_command', is_error: false, call_id: 'call-1' },
		},
		text: '.',
	});

	assert.equal(isBoilerplateEvent(dotResult), true);
});

test('isBoilerplateEvent does not hide empty exec results', () => {
	const emptyResult = textEvent({
		id: 'e1',
		type: {
			type: 'ToolResult',
			data: { name: 'exec_command', is_error: false, call_id: 'call-1' },
		},
	});

	assert.equal(isBoilerplateEvent(emptyResult), false);
});

test('isBoilerplateEvent keeps exec output when low-signal markers precede real content', () => {
	const mixedResult: Event = {
		...textEvent({
			id: 'e2',
			type: {
				type: 'ToolResult',
				data: { name: 'exec_command', is_error: false, call_id: 'call-2' },
			},
		}),
		content: {
			blocks: [{ type: 'Code', code: '.\ncompleted: updated 1 file', language: 'text', start_line: 1 }],
		},
	};

	assert.equal(isBoilerplateEvent(mixedResult), false);
});

test('prepareTimelineEvents coalesces consecutive duplicate thinking rows', () => {
	const events: Event[] = [
		textEvent({
			id: 't1',
			type: { type: 'Thinking' },
			text: '**Inspecting store and URL files**',
		}),
		textEvent({
			id: 't2',
			type: { type: 'Thinking' },
			text: '**Inspecting store and URL files**',
		}),
	];

	const prepared = prepareTimelineEvents(events);
	assert.equal(prepared.length, 1);
	assert.equal(prepared[0].event_type.type, 'Thinking');
});

test('prepareTimelineEvents keeps similarly-shaped tool results when call_id differs', () => {
	const events: Event[] = [
		textEvent({
			id: 'r1',
			type: {
				type: 'ToolResult',
				data: { name: 'exec_command', is_error: false, call_id: 'call-1' },
			},
			text: 'Chunk ID: abc123',
		}),
		textEvent({
			id: 'r2',
			type: {
				type: 'ToolResult',
				data: { name: 'exec_command', is_error: false, call_id: 'call-2' },
			},
			text: 'Chunk ID: abc123',
		}),
	];

	const prepared = prepareTimelineEvents(events);
	assert.equal(prepared.length, 2);
});

test('prepareTimelineEvents coalesces duplicate thinking rows even when task differs', () => {
	const events: Event[] = [
		textEvent({
			id: 't1',
			type: { type: 'Thinking' },
			task_id: 'task-a',
			text: '**Inspecting store and URL files**',
		}),
		textEvent({
			id: 't2',
			type: { type: 'Thinking' },
			task_id: 'task-b',
			text: '**Inspecting store and URL files**',
		}),
	];

	const prepared = prepareTimelineEvents(events);
	assert.equal(prepared.length, 1);
});

test('prepareTimelineEvents filters low-signal custom dot events', () => {
	const events: Event[] = [
		textEvent({
			id: 'c1',
			type: { type: 'Custom', data: { kind: 'adapter.progress' } },
			text: '.',
		}),
		textEvent({
			id: 'a1',
			type: { type: 'AgentMessage' },
			text: 'real response',
		}),
	];

	const prepared = prepareTimelineEvents(events);
	assert.deepEqual(
		prepared.map((event) => event.event_id),
		['a1'],
	);
});

test('prepareTimelineEvents filters low-signal user dot events', () => {
	const events: Event[] = [
		textEvent({
			id: 'u-dot',
			type: { type: 'UserMessage' },
			text: '.',
		}),
		textEvent({
			id: 'agent-ok',
			type: { type: 'AgentMessage' },
			text: 'real response',
		}),
	];

	const prepared = prepareTimelineEvents(events);
	assert.deepEqual(
		prepared.map((event) => event.event_id),
		['agent-ok'],
	);
});

test('prepareTimelineEvents filters low-signal tool result dot events in code blocks', () => {
	const events: Event[] = [
		{
			event_id: 'r-dot',
			timestamp: '2026-03-03T00:00:00Z',
			event_type: {
				type: 'ToolResult',
				data: { name: 'exec_command', is_error: false, call_id: 'call-dot' },
			},
			task_id: null,
			content: {
				blocks: [{ type: 'Code', code: '.', language: 'text', start_line: 1 }],
			},
			duration_ms: undefined,
			attributes: {},
		},
		textEvent({
			id: 'agent-ok',
			type: { type: 'AgentMessage' },
			text: 'real response',
		}),
	];

	const prepared = prepareTimelineEvents(events);
	assert.deepEqual(
		prepared.map((event) => event.event_id),
		['agent-ok'],
	);
});

test('prepareTimelineEvents filters low-signal tool call dot events', () => {
	const events: Event[] = [
		textEvent({
			id: 'call-dot',
			type: {
				type: 'ToolCall',
				data: { name: 'unknown_tool' },
			},
			text: '.',
		}),
		textEvent({
			id: 'result-ok',
			type: {
				type: 'ToolResult',
				data: { name: 'exec_command', is_error: false, call_id: 'call-1' },
			},
			text: 'Chunk ID: 6978d4',
		}),
	];

	const prepared = prepareTimelineEvents(events);
	assert.deepEqual(
		prepared.map((event) => event.event_id),
		['result-ok'],
	);
});

test('prepareTimelineEvents keeps tool result code blocks when they include meaningful lines', () => {
	const events: Event[] = [
		{
			event_id: 'r-meaningful',
			timestamp: '2026-03-03T00:00:00Z',
			event_type: {
				type: 'ToolResult',
				data: { name: 'exec_command', is_error: false, call_id: 'call-meaningful' },
			},
			task_id: null,
			content: {
				blocks: [{ type: 'Code', code: '.\ncompleted: 1 file updated', language: 'text', start_line: 1 }],
			},
			duration_ms: undefined,
			attributes: {},
		},
	];

	const prepared = prepareTimelineEvents(events);
	assert.equal(prepared.length, 1);
	assert.equal(prepared[0].event_id, 'r-meaningful');
});

test('pairToolCallResults matches semantic.call_id and ignores legacy attrs.call_id', () => {
	const events: Event[] = [
		{
			...textEvent({
				id: 'call',
				type: { type: 'ToolCall', data: { name: 'exec_command' } },
			}),
			attributes: {
				'semantic.call_id': 'cid-1',
				call_id: 'legacy-cid-should-be-ignored',
			},
		},
		textEvent({
			id: 'result',
			type: {
				type: 'ToolResult',
				data: { name: 'exec_command', is_error: false, call_id: 'cid-1' },
			},
			text: 'ok',
		}),
	];

	const pairs = pairToolCallResults(events);
	assert.equal(pairs.get(0)?.event_id, 'result');
});

test('pairToolCallResults does not use legacy attrs.call_id for direct matching', () => {
	const events: Event[] = [
		{
			...textEvent({
				id: 'call',
				type: { type: 'ToolCall', data: { name: 'exec_command' } },
			}),
			attributes: {
				call_id: 'legacy-only-id',
			},
		},
		{
			...textEvent({
				id: 'result',
				type: { type: 'ToolResult', data: { name: 'different_tool', is_error: false } },
				text: 'ok',
			}),
			attributes: {
				call_id: 'legacy-only-id',
			},
		},
	];

	const pairs = pairToolCallResults(events);
	assert.equal(pairs.has(0), false);
});

test('extractFileEditDiff returns direct diff payload when present', () => {
	const event = textEvent({
		id: 'file-edit-direct',
		type: {
			type: 'FileEdit',
			data: {
				path: 'src/app.ts',
				diff: '@@ -1 +1 @@\n-old\n+new',
			},
		},
	});

	assert.equal(extractFileEditDiff(event), '@@ -1 +1 @@\n-old\n+new');
});

test('extractFileEditDiff converts apply_patch json payload into unified diff', () => {
	const event: Event = {
		...textEvent({
			id: 'file-edit-patch',
			type: {
				type: 'FileEdit',
				data: {
					path: 'src/app.ts',
				},
			},
		}),
		content: {
			blocks: [
				{
					type: 'Json',
					data: {
						input: [
							'*** Begin Patch',
							'*** Update File: src/app.ts',
							'@@ -1,2 +1,2 @@',
							'-const value = 1;',
							'+const value = 2;',
							' console.log(value);',
							'*** End Patch',
						].join('\n'),
					},
				},
			],
		},
	};

	const diff = extractFileEditDiff(event);
	assert.ok(diff);
	assert.match(diff, /diff --git a\/src\/app\.ts b\/src\/app\.ts/);
	assert.match(diff, /@@ -1,2 \+1,2 @@/);
	assert.match(diff, /\+const value = 2;/);
	assert.match(diff, /-const value = 1;/);
});
