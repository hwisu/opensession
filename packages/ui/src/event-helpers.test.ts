import assert from 'node:assert/strict';
import test from 'node:test';
import { isBoilerplateEvent, prepareTimelineEvents } from './event-helpers';
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
		type: { type: 'ToolResult', data: { name: 'exec_command', is_error: false, call_id: 'call-1' } },
		text: '.',
	});

	assert.equal(isBoilerplateEvent(dotResult), true);
});

test('isBoilerplateEvent does not hide empty exec results', () => {
	const emptyResult = textEvent({
		id: 'e1',
		type: { type: 'ToolResult', data: { name: 'exec_command', is_error: false, call_id: 'call-1' } },
	});

	assert.equal(isBoilerplateEvent(emptyResult), false);
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
			type: { type: 'ToolResult', data: { name: 'exec_command', is_error: false, call_id: 'call-1' } },
			text: 'Chunk ID: abc123',
		}),
		textEvent({
			id: 'r2',
			type: { type: 'ToolResult', data: { name: 'exec_command', is_error: false, call_id: 'call-2' } },
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
