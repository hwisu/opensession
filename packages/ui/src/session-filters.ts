import type { Event } from './types';

export type SessionViewMode = 'unified' | 'native';

export interface FilterOption {
	key: string;
	label: string;
	count: number;
}

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

function toCustomUnifiedFilterKey(kind: string): string {
	return `Custom:${kind}`;
}

function unifiedLabelForKey(key: string): string {
	if (key.startsWith('Custom:')) {
		const kind = key.slice('Custom:'.length);
		return `Custom:${kind}`;
	}
	return key;
}

function nativeLabelForKey(key: string): string {
	return NATIVE_GROUP_LABELS[key] ?? key;
}

export function unifiedFilterKeyForEvent(event: Event): string {
	const eventType = event.event_type;
	if (eventType.type === 'Custom') {
		return toCustomUnifiedFilterKey(eventType.data.kind);
	}
	return eventType.type;
}

export function buildUnifiedFilterOptions(events: Event[]): FilterOption[] {
	const counts = new Map<string, number>();
	for (const event of events) {
		increment(counts, unifiedFilterKeyForEvent(event));
	}
	return sortFilterOptions(counts, unifiedLabelForKey);
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
	return sortFilterOptions(counts, nativeLabelForKey);
}

export function filterEventsByUnifiedKeys(events: Event[], enabledKeys: Set<string>): Event[] {
	if (enabledKeys.size === 0) return [];
	return events.filter((event) => enabledKeys.has(unifiedFilterKeyForEvent(event)));
}

export function filterEventsByNativeGroups(events: Event[], enabledGroups: Set<string>): Event[] {
	if (enabledGroups.size === 0) return [];
	return events.filter((event) => enabledGroups.has(nativeGroupForEvent(event)));
}
