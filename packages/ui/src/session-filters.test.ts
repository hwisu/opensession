import assert from 'node:assert/strict';
import test from 'node:test';
import {
	buildBranchpointFilterOptions,
	buildNativeFilterOptions,
	buildUnifiedFilterOptions,
	filterEventsByBranchpointKeys,
	filterEventsByUnifiedKeys,
	nativeGroupForEvent,
	branchpointFilterKeyForEvent,
	toggleAllBackedFilter,
	unifiedFilterKeyForEvent,
} from './session-filters';
import type { Event } from './types';

function textEvent(id: string, eventType: Event['event_type']): Event {
	return {
		event_id: id,
		timestamp: '2026-03-03T00:00:00Z',
		event_type: eventType,
		task_id: undefined,
		content: { blocks: [{ type: 'Text', text: id }] },
		duration_ms: undefined,
		attributes: {},
	};
}

test('buildUnifiedFilterOptions returns fixed semantic order with counts', () => {
	const events: Event[] = [
		textEvent('u1', { type: 'UserMessage' }),
		textEvent('a1', { type: 'AgentMessage' }),
		textEvent('t1', { type: 'Thinking' }),
		textEvent('tool1', { type: 'ToolCall', data: { name: 'exec_command' } }),
		textEvent('file1', { type: 'FileRead', data: { path: 'README.md' } }),
		textEvent('shell1', { type: 'ShellCommand', data: { command: 'ls -la' } }),
		textEvent('task1', { type: 'TaskStart', data: { title: 'run checks' } }),
		textEvent('web1', { type: 'WebFetch', data: { url: 'https://example.com' } }),
		textEvent('other1', { type: 'ImageGenerate', data: { prompt: 'cat' } }),
	];

	const options = buildUnifiedFilterOptions(events);

	assert.deepEqual(
		options.map((option) => option.key),
		['all', 'user', 'agent', 'think', 'tools', 'files', 'shell', 'task', 'web', 'other'],
	);
	assert.equal(options.find((option) => option.key === 'all')?.count, events.length);
	assert.equal(options.find((option) => option.key === 'user')?.count, 1);
	assert.equal(options.find((option) => option.key === 'agent')?.count, 1);
	assert.equal(options.find((option) => option.key === 'think')?.count, 1);
	assert.equal(options.find((option) => option.key === 'tools')?.count, 1);
	assert.equal(options.find((option) => option.key === 'files')?.count, 1);
	assert.equal(options.find((option) => option.key === 'shell')?.count, 1);
	assert.equal(options.find((option) => option.key === 'task')?.count, 1);
	assert.equal(options.find((option) => option.key === 'web')?.count, 1);
	assert.equal(options.find((option) => option.key === 'other')?.count, 1);
});

test('filterEventsByUnifiedKeys keeps all events when all is selected', () => {
	const events: Event[] = [
		textEvent('u1', { type: 'UserMessage' }),
		textEvent('a1', { type: 'AgentMessage' }),
		textEvent('tool1', { type: 'ToolResult', data: { name: 'exec_command', is_error: false } }),
	];

	const filtered = filterEventsByUnifiedKeys(events, new Set(['all']));
	assert.equal(filtered.length, events.length);
	assert.deepEqual(
		filtered.map((event) => event.event_id),
		events.map((event) => event.event_id),
	);
});

test('filterEventsByUnifiedKeys filters by semantic categories', () => {
	const events: Event[] = [
		textEvent('u1', { type: 'UserMessage' }),
		textEvent('s1', { type: 'SystemMessage' }),
		textEvent('a1', { type: 'AgentMessage' }),
		textEvent('tool1', { type: 'ToolCall', data: { name: 'exec_command' } }),
	];

	const userOnly = filterEventsByUnifiedKeys(events, new Set(['user']));
	assert.deepEqual(
		userOnly.map((event) => event.event_id),
		['u1', 's1'],
	);

	const agentAndTools = filterEventsByUnifiedKeys(events, new Set(['agent', 'tools']));
	assert.deepEqual(
		agentAndTools.map((event) => event.event_id),
		['a1', 'tool1'],
	);
});

test('unifiedFilterKeyForEvent maps unmatched types to other bucket', () => {
	const event = textEvent('custom1', { type: 'Custom', data: { kind: 'adapter.progress' } });
	assert.equal(unifiedFilterKeyForEvent(event), 'other');
});

test('branchpoint mode counts only semantic branch events', () => {
	const events: Event[] = [
		textEvent('u1', { type: 'UserMessage' }),
		textEvent('a1', { type: 'AgentMessage' }),
		textEvent('s1', { type: 'SystemMessage' }),
		textEvent('task1', { type: 'TaskStart', data: { title: 'run tests' } }),
		textEvent('ok1', { type: 'ToolResult', data: { name: 'exec_command', is_error: false } }),
		textEvent('err1', { type: 'ToolResult', data: { name: 'exec_command', is_error: true } }),
		textEvent('err2', { type: 'ShellCommand', data: { command: 'exit 1', exit_code: 1 } }),
		textEvent('file1', { type: 'FileRead', data: { path: 'README.md' } }),
	];

	const options = buildBranchpointFilterOptions(events);
	assert.deepEqual(
		options.map((option) => option.key),
		['all', 'question', 'answer', 'system', 'task', 'error'],
	);
	assert.equal(options.find((option) => option.key === 'all')?.count, 6);
	assert.equal(options.find((option) => option.key === 'question')?.count, 1);
	assert.equal(options.find((option) => option.key === 'answer')?.count, 1);
	assert.equal(options.find((option) => option.key === 'system')?.count, 1);
	assert.equal(options.find((option) => option.key === 'task')?.count, 1);
	assert.equal(options.find((option) => option.key === 'error')?.count, 2);
});

test('branchpoint mode filters to meaningful path nodes only', () => {
	const events: Event[] = [
		textEvent('u1', { type: 'UserMessage' }),
		textEvent('a1', { type: 'AgentMessage' }),
		textEvent('s1', { type: 'SystemMessage' }),
		textEvent('task1', { type: 'TaskEnd', data: { summary: 'done' } }),
		textEvent('err1', { type: 'ToolResult', data: { name: 'exec_command', is_error: true } }),
		textEvent('file1', { type: 'FileEdit', data: { path: 'src/app.ts' } }),
	];

	const allBranch = filterEventsByBranchpointKeys(events, new Set(['all']));
	assert.deepEqual(
		allBranch.map((event) => event.event_id),
		['u1', 'a1', 's1', 'task1', 'err1'],
	);

	const questionAndError = filterEventsByBranchpointKeys(events, new Set(['question', 'error']));
	assert.deepEqual(
		questionAndError.map((event) => event.event_id),
		['u1', 'err1'],
	);
});

test('native grouping remains bounded to keyboard slots', () => {
	const events: Event[] = [
		textEvent('msg', { type: 'UserMessage' }),
		textEvent('tool', { type: 'ToolCall', data: { name: 'exec_command' } }),
		textEvent('file', { type: 'FileRead', data: { path: 'README.md' } }),
		textEvent('reasoning', { type: 'Thinking' }),
		textEvent('shell', { type: 'ShellCommand', data: { command: 'ls' } }),
		textEvent('task', { type: 'TaskStart', data: { title: 'task' } }),
		textEvent('web', { type: 'WebFetch', data: { url: 'https://example.com' } }),
		textEvent('media', { type: 'ImageGenerate', data: { prompt: 'cat' } }),
		textEvent('custom', { type: 'Custom', data: { kind: 'adapter.progress' } }),
		textEvent('other', { type: 'VideoGenerate', data: { prompt: 'clip' } }),
	];

	const options = buildNativeFilterOptions(events);
	assert.ok(options.length <= 10);
	assert.equal(nativeGroupForEvent(events[0]), 'message');
	assert.equal(branchpointFilterKeyForEvent(events[0]), 'question');
});

test('toggleAllBackedFilter keeps all-selection semantics consistent', () => {
	assert.deepEqual(
		Array.from(toggleAllBackedFilter(new Set(['all']), 'user')).sort(),
		['user'],
	);
	assert.deepEqual(
		Array.from(toggleAllBackedFilter(new Set(['user', 'agent']), 'user')).sort(),
		['agent'],
	);
	assert.deepEqual(
		Array.from(toggleAllBackedFilter(new Set(['user']), 'user')).sort(),
		['all'],
	);
});

test('branchpoint mode maps interactive source events to semantic question/answer', () => {
	const systemQuestion: Event = {
		...textEvent('sys-q', { type: 'SystemMessage' }),
		attributes: { source: 'interactive_question' },
	};
	const userAnswer: Event = {
		...textEvent('user-a', { type: 'UserMessage' }),
		attributes: { source: 'interactive' },
	};

	assert.equal(branchpointFilterKeyForEvent(systemQuestion), 'question');
	assert.equal(branchpointFilterKeyForEvent(userAnswer), 'answer');
});

test('branchpoint mode maps turn_aborted custom events to error', () => {
	const turnAborted = textEvent('abort', { type: 'Custom', data: { kind: 'turn_aborted' } });
	assert.equal(branchpointFilterKeyForEvent(turnAborted), 'error');
});
