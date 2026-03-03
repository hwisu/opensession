import type { Event } from './types';

export type SessionViewMode = 'unified' | 'branch' | 'native';

export interface FilterOption {
	key: string;
	label: string;
	count: number;
}

type UnifiedSemanticFilterKey =
	| 'all'
	| 'user'
	| 'agent'
	| 'think'
	| 'tools'
	| 'files'
	| 'shell'
	| 'task'
	| 'web'
	| 'other';

type BranchpointFilterKey = 'all' | 'question' | 'answer' | 'system' | 'task' | 'error';

const UNIFIED_SEMANTIC_FILTERS: ReadonlyArray<{ key: UnifiedSemanticFilterKey; label: string }> = [
	{ key: 'all', label: 'All' },
	{ key: 'user', label: 'User' },
	{ key: 'agent', label: 'Agent' },
	{ key: 'think', label: 'Thinking' },
	{ key: 'tools', label: 'Tools' },
	{ key: 'files', label: 'Files' },
	{ key: 'shell', label: 'Shell' },
	{ key: 'task', label: 'Task' },
	{ key: 'web', label: 'Web' },
	{ key: 'other', label: 'Other' },
];

const BRANCHPOINT_FILTERS: ReadonlyArray<{ key: BranchpointFilterKey; label: string }> = [
	{ key: 'all', label: 'All' },
	{ key: 'question', label: 'Questions' },
	{ key: 'answer', label: 'Answers' },
	{ key: 'system', label: 'System' },
	{ key: 'task', label: 'Task' },
	{ key: 'error', label: 'Errors' },
];

const NATIVE_GROUP_LABELS: Record<string, string> = {
	message: 'Messages',
	tool: 'Tool Calls',
	file: 'File Events',
	reasoning: 'Reasoning',
	shell: 'Shell',
	task: 'Tasks',
	web: 'Web',
	media: 'Media',
	custom: 'Custom',
	other: 'Other',
};

const NATIVE_ADAPTERS = new Set([
	'codex',
	'claude-code',
	'gemini',
	'amp',
	'cline',
	'cursor',
	'opencode',
]);

const MAX_FILTER_SLOTS = 10;

function increment(counts: Map<string, number>, key: string) {
	counts.set(key, (counts.get(key) ?? 0) + 1);
}

function sortFilterOptions(counts: Map<string, number>, labelFor: (key: string) => string): FilterOption[] {
	return Array.from(counts.entries())
		.sort((a, b) => {
			if (b[1] !== a[1]) return b[1] - a[1];
			return a[0].localeCompare(b[0]);
		})
		.map(([key, count]) => ({
			key,
			label: labelFor(key),
			count,
		}));
}

function nativeLabelForKey(key: string): string {
	return NATIVE_GROUP_LABELS[key] ?? key;
}

export function unifiedFilterKeyForEvent(event: Event): UnifiedSemanticFilterKey {
	switch (event.event_type.type) {
		case 'UserMessage':
		case 'SystemMessage':
			return 'user';
		case 'AgentMessage':
			return 'agent';
		case 'Thinking':
			return 'think';
		case 'ToolCall':
		case 'ToolResult':
			return 'tools';
		case 'FileRead':
		case 'FileEdit':
		case 'FileCreate':
		case 'FileDelete':
		case 'FileSearch':
		case 'CodeSearch':
			return 'files';
		case 'ShellCommand':
			return 'shell';
		case 'TaskStart':
		case 'TaskEnd':
			return 'task';
		case 'WebSearch':
		case 'WebFetch':
			return 'web';
		default:
			return 'other';
	}
}

export function buildUnifiedFilterOptions(events: Event[]): FilterOption[] {
	const counts = new Map<UnifiedSemanticFilterKey, number>();
	for (const filter of UNIFIED_SEMANTIC_FILTERS) {
		counts.set(filter.key, 0);
	}
	counts.set('all', events.length);
	for (const event of events) {
		increment(counts, unifiedFilterKeyForEvent(event));
	}
	return UNIFIED_SEMANTIC_FILTERS.map((filter) => ({
		key: filter.key,
		label: filter.label,
		count: counts.get(filter.key) ?? 0,
	}));
}

export function branchpointFilterKeyForEvent(event: Event): BranchpointFilterKey | null {
	const source = String(event.attributes?.source ?? '').toLowerCase();
	switch (event.event_type.type) {
		case 'UserMessage':
			if (source === 'interactive') return 'answer';
			return 'question';
		case 'AgentMessage':
			return 'answer';
		case 'SystemMessage':
			if (source === 'interactive_question') return 'question';
			return 'system';
		case 'TaskStart':
		case 'TaskEnd':
			return 'task';
		case 'ToolResult':
			return event.event_type.data.is_error ? 'error' : null;
		case 'ShellCommand': {
			const exitCode = event.event_type.data.exit_code;
			return typeof exitCode === 'number' && exitCode !== 0 ? 'error' : null;
		}
		case 'Custom': {
			const kind = String(event.event_type.data.kind ?? '').toLowerCase();
			if (kind === 'turn_aborted' || kind.includes('error')) return 'error';
			return null;
		}
		default:
			return null;
	}
}

export function buildBranchpointFilterOptions(events: Event[]): FilterOption[] {
	const counts = new Map<BranchpointFilterKey, number>();
	for (const filter of BRANCHPOINT_FILTERS) {
		counts.set(filter.key, 0);
	}
	for (const event of events) {
		const key = branchpointFilterKeyForEvent(event);
		if (!key) continue;
		increment(counts, 'all');
		increment(counts, key);
	}
	return BRANCHPOINT_FILTERS.map((filter) => ({
		key: filter.key,
		label: filter.label,
		count: counts.get(filter.key) ?? 0,
	}));
}

export function isNativeAdapterSupported(adapter: string | null | undefined): boolean {
	if (!adapter) return false;
	return NATIVE_ADAPTERS.has(adapter);
}

export function nativeGroupForEvent(event: Event): string {
	switch (event.event_type.type) {
		case 'UserMessage':
		case 'AgentMessage':
		case 'SystemMessage':
			return 'message';
		case 'ToolCall':
		case 'ToolResult':
			return 'tool';
		case 'FileRead':
		case 'FileEdit':
		case 'FileCreate':
		case 'FileDelete':
		case 'FileSearch':
		case 'CodeSearch':
			return 'file';
		case 'Thinking':
			return 'reasoning';
		case 'ShellCommand':
			return 'shell';
		case 'TaskStart':
		case 'TaskEnd':
			return 'task';
		case 'WebSearch':
		case 'WebFetch':
			return 'web';
		case 'ImageGenerate':
		case 'VideoGenerate':
		case 'AudioGenerate':
			return 'media';
		case 'Custom':
			return 'custom';
		default:
			return 'other';
	}
}

export function buildNativeFilterOptions(events: Event[]): FilterOption[] {
	const counts = new Map<string, number>();
	for (const event of events) {
		increment(counts, nativeGroupForEvent(event));
	}
	const sorted = sortFilterOptions(counts, nativeLabelForKey);
	if (sorted.length <= MAX_FILTER_SLOTS) {
		return sorted;
	}

	const top = sorted.slice(0, MAX_FILTER_SLOTS - 1);
	const overflowCount = sorted
		.slice(MAX_FILTER_SLOTS - 1)
		.reduce((sum, option) => sum + option.count, 0);
	const otherInTop = top.find((option) => option.key === 'other');
	if (otherInTop) {
		otherInTop.count += overflowCount;
		return top;
	}
	return [...top, { key: 'other', label: nativeLabelForKey('other'), count: overflowCount }];
}

export function filterEventsByUnifiedKeys(events: Event[], enabledKeys: Set<string>): Event[] {
	if (enabledKeys.size === 0) return [];
	if (enabledKeys.has('all')) return events;
	return events.filter((event) => enabledKeys.has(unifiedFilterKeyForEvent(event)));
}

export function filterEventsByNativeGroups(events: Event[], enabledGroups: Set<string>): Event[] {
	if (enabledGroups.size === 0) return [];
	return events.filter((event) => enabledGroups.has(nativeGroupForEvent(event)));
}

export function filterEventsByBranchpointKeys(events: Event[], enabledKeys: Set<string>): Event[] {
	if (enabledKeys.size === 0) return [];
	const branchEvents = events.filter((event) => branchpointFilterKeyForEvent(event) != null);
	if (enabledKeys.has('all')) return branchEvents;
	return branchEvents.filter((event) => {
		const key = branchpointFilterKeyForEvent(event);
		return key != null && enabledKeys.has(key);
	});
}

export function toggleAllBackedFilter(current: Set<string>, key: string, allKey = 'all'): Set<string> {
	if (key === allKey) {
		return new Set([allKey]);
	}
	const next = new Set(current);
	next.delete(allKey);
	if (next.has(key)) next.delete(key);
	else next.add(key);
	if (next.size === 0) {
		next.add(allKey);
	}
	return next;
}
