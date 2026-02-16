import { TRUNCATE_LENGTH } from './constants';
import type { ContentBlock, Event, EventType } from './types';

/** Extract tool name from ToolCall/ToolResult event types */
export function getToolName(eventType: EventType): string {
	if ('data' in eventType && 'name' in eventType.data) {
		return String(eventType.data.name);
	}
	return eventType.type;
}

/** Check if a ToolResult event is an error */
export function isToolError(eventType: EventType): boolean {
	return (
		eventType.type === 'ToolResult' &&
		'data' in eventType &&
		(eventType.data as { is_error: boolean }).is_error
	);
}

/** Truncate text to max length, appending '...' if needed */
export function truncate(text: string, max: number = TRUNCATE_LENGTH): string {
	if (text.length <= max) return text;
	return `${text.slice(0, max - 3)}...`;
}

/** Format content length as "3k" / "150" */
export function formatContentLength(len: number): string {
	return len > 1000 ? `${Math.round(len / 1000)}k` : String(len);
}

/** Calculate total text length across all blocks */
export function calcContentLength(blocks: ContentBlock[]): number {
	let len = 0;
	for (const block of blocks) {
		if (block.type === 'Text') len += block.text.length;
		else if (block.type === 'Code') len += block.code.length;
	}
	return len;
}

/** Find first Code block and return its stats */
export function findCodeStats(
	blocks: ContentBlock[],
): { lines: number; lang?: string; startLine: number } | null {
	for (const block of blocks) {
		if (block.type === 'Code') {
			return {
				lines: block.code.split('\n').length,
				lang: block.language,
				startLine: block.start_line ?? 1,
			};
		}
	}
	return null;
}

/** Find first Text block's text */
export function findFirstText(blocks: ContentBlock[]): string | null {
	for (const block of blocks) {
		if (block.type === 'Text') return block.text;
	}
	return null;
}

/** Find first Json block's data */
export function findJsonPayload(blocks: ContentBlock[]): Record<string, unknown> | null {
	for (const block of blocks) {
		if (block.type === 'Json' && block.data && typeof block.data === 'object') {
			return block.data as Record<string, unknown>;
		}
	}
	return null;
}

function blockTextFragments(block: ContentBlock): string[] {
	if (block.type === 'Text') return [block.text];
	if (block.type === 'Code') return [block.code];
	if (block.type === 'Json') return [JSON.stringify(block.data)];
	return [];
}

function firstEventTextLine(event: Event): string | null {
	for (const block of event.content.blocks) {
		for (const fragment of blockTextFragments(block)) {
			for (const line of fragment.split('\n')) {
				const trimmed = line.trim();
				if (trimmed.length > 0) return trimmed;
			}
		}
	}
	return null;
}

function isRunningSessionStatusLine(line: string | null): boolean {
	if (!line) return true;
	const lowered = line.trim().toLowerCase();
	return (
		lowered.length === 0 ||
		lowered.includes('process running with session id') ||
		lowered === 'ok' ||
		lowered === 'output:'
	);
}

function isMarkdownProgressLine(line: string): boolean {
	const trimmed = line.trim();
	if (!(trimmed.startsWith('**') && trimmed.endsWith('**') && trimmed.length > 4)) return false;
	const lowered = trimmed.toLowerCase();
	return (
		lowered.includes('evaluating') ||
		lowered.includes('planning') ||
		lowered.includes('adjusting') ||
		lowered.includes('confirming') ||
		lowered.includes('summarizing')
	);
}

/** Matches TUI boilerplate filtering so Web/TUI show the same primary timeline. */
export function isBoilerplateEvent(event: Event): boolean {
	const typeName = event.event_type.type;
	if (typeName === 'ToolCall' && getToolName(event.event_type).toLowerCase() === 'write_stdin') {
		return true;
	}
	if (typeName === 'ToolResult') {
		const tool = getToolName(event.event_type).toLowerCase();
		if (tool === 'write_stdin') {
			return isRunningSessionStatusLine(firstEventTextLine(event));
		}
		if (['exec_command', 'shell', 'bash', 'execute_command', 'spawn_process'].includes(tool)) {
			return isRunningSessionStatusLine(firstEventTextLine(event));
		}
	}
	if (typeName === 'Thinking') {
		const line = firstEventTextLine(event);
		return line ? isMarkdownProgressLine(line) : false;
	}
	return false;
}

function semanticCallId(event: Event): string | null {
	const attrs = event.attributes ?? {};
	const fromSemantic = attrs['semantic.call_id'];
	if (typeof fromSemantic === 'string' && fromSemantic.trim().length > 0) return fromSemantic;
	if (event.event_type.type === 'ToolResult') {
		const fromType = event.event_type.data.call_id;
		if (typeof fromType === 'string' && fromType.trim().length > 0) return fromType;
	}
	const fromLegacy = attrs.call_id;
	return typeof fromLegacy === 'string' && fromLegacy.trim().length > 0 ? fromLegacy : null;
}

/** Pair ToolCall rows with ToolResult rows for stable status coloring/chips. */
export function pairToolCallResults(events: Event[]): Map<number, Event> {
	const pairs = new Map<number, Event>();
	const resultByCallId = new Map<string, Event>();

	for (const event of events) {
		if (event.event_type.type !== 'ToolResult') continue;
		const callId = semanticCallId(event);
		if (callId) resultByCallId.set(callId, event);
	}

	for (let i = 0; i < events.length; i++) {
		const event = events[i];
		if (event.event_type.type !== 'ToolCall') continue;

		const callId = semanticCallId(event) ?? event.event_id;
		const direct = resultByCallId.get(callId);
		if (direct) {
			pairs.set(i, direct);
			continue;
		}

		const toolName = getToolName(event.event_type);
		for (let j = i + 1; j < Math.min(events.length, i + 8); j++) {
			const candidate = events[j];
			if (candidate.event_type.type === 'ToolCall') break;
			if (
				candidate.event_type.type === 'ToolResult' &&
				getToolName(candidate.event_type) === toolName
			) {
				pairs.set(i, candidate);
				break;
			}
		}
	}

	return pairs;
}
