import { TRUNCATE_LENGTH } from './constants';
import type { ContentBlock, Event, EventType } from './types';

const LOW_SIGNAL_LINE_RE = /^[.\u00B7\u2022\-_=~`]+$/;

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

function eventTextLines(event: Event): string[] {
	const out: string[] = [];
	for (const block of event.content.blocks) {
		for (const fragment of blockTextFragments(block)) {
			for (const line of fragment.split('\n')) {
				const trimmed = line.trim();
				if (trimmed.length > 0) out.push(trimmed);
			}
		}
	}
	return out;
}

function firstEventTextLine(event: Event): string | null {
	return eventTextLines(event)[0] ?? null;
}

function isLowSignalLine(line: string): boolean {
	const trimmed = line.trim();
	return trimmed.length > 0 && trimmed.length <= 8 && LOW_SIGNAL_LINE_RE.test(trimmed);
}

/** First non-empty text line that is not a keepalive marker (".", "...", "----"). */
export function firstMeaningfulEventLine(event: Event): string | null {
	for (const line of eventTextLines(event)) {
		if (!isLowSignalLine(line)) return line;
	}
	return null;
}

function isRunningSessionStatusLine(line: string | null): boolean {
	if (!line) return false;
	const lowered = line.trim().toLowerCase();
	return (
		lowered.length === 0 ||
		lowered.includes('process running with session id') ||
		lowered === 'ok' ||
		lowered === 'output:' ||
		isLowSignalLine(lowered)
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

function stableStringify(value: unknown): string {
	if (value == null) return 'null';
	if (typeof value === 'string') return JSON.stringify(value);
	if (typeof value === 'number' || typeof value === 'boolean') return String(value);
	if (Array.isArray(value)) {
		return `[${value.map((item) => stableStringify(item)).join(',')}]`;
	}
	if (typeof value === 'object') {
		const obj = value as Record<string, unknown>;
		const keys = Object.keys(obj).sort();
		const pairs = keys.map((key) => `${JSON.stringify(key)}:${stableStringify(obj[key])}`);
		return `{${pairs.join(',')}}`;
	}
	return JSON.stringify(String(value));
}

function normalizeTextForSignature(text: string): string {
	return text.replace(/\r\n?/g, '\n').trim();
}

function blockSignature(block: ContentBlock): string {
	switch (block.type) {
		case 'Text':
			return `Text:${normalizeTextForSignature(block.text)}`;
		case 'Code':
			return `Code:${block.language ?? ''}:${normalizeTextForSignature(block.code)}`;
		case 'Json':
			return `Json:${stableStringify(block.data)}`;
		case 'File':
			return `File:${block.path}:${normalizeTextForSignature(block.content ?? '')}`;
		case 'Image':
			return `Image:${block.url}:${block.alt ?? ''}:${block.mime}`;
		case 'Video':
			return `Video:${block.url}:${block.mime}`;
		case 'Audio':
			return `Audio:${block.url}:${block.mime}`;
		case 'Reference':
			return `Reference:${block.uri}:${block.media_type}`;
	}
}

function eventSignature(event: Event): string {
	const typeData = 'data' in event.event_type ? stableStringify(event.event_type.data) : '';
	const contentSignature = event.content.blocks.map((block) => blockSignature(block)).join('\u241e');
	const stableTaskId = event.event_type.type === 'Thinking' ? '' : (event.task_id ?? '');
	return [
		event.event_type.type,
		stableTaskId,
		semanticCallId(event) ?? '',
		typeData,
		contentSignature,
	].join('\u241f');
}

function eventInfoScore(event: Event): number {
	let textLen = 0;
	for (const block of event.content.blocks) {
		if (block.type === 'Text') textLen += block.text.length;
		else if (block.type === 'Code') textLen += block.code.length;
	}
	return (
		event.content.blocks.length * 4 +
		Object.keys(event.attributes ?? {}).length * 2 +
		Math.min(64, textLen)
	);
}

function shouldCoalesceConsecutiveEvents(previous: Event, current: Event): boolean {
	return eventSignature(previous) === eventSignature(current);
}

function pickPreferredDuplicate(previous: Event, current: Event): Event {
	const prevScore = eventInfoScore(previous);
	const currScore = eventInfoScore(current);
	if (currScore > prevScore) return current;
	return previous;
}

function isLowSignalStandaloneEvent(event: Event): boolean {
	const typeName = event.event_type.type;
	if (
		typeName !== 'AgentMessage' &&
		typeName !== 'Thinking' &&
		typeName !== 'ToolResult' &&
		typeName !== 'SystemMessage' &&
		typeName !== 'Custom'
	) {
		return false;
	}
	if (typeName === 'ToolResult' && isToolError(event.event_type)) return false;
	if (event.content.blocks.length === 0) return false;
	if (!event.content.blocks.every((block) => block.type === 'Text')) return false;
	const lines = eventTextLines(event);
	if (lines.length === 0) return false;
	return lines.every((line) => isLowSignalLine(line));
}

/**
 * Canonical timeline projection used by web/desktop rendering:
 * - removes boilerplate transport noise
 * - removes low-signal keepalive rows (".", "...")
 * - coalesces adjacent duplicate events with identical semantic payload
 */
export function prepareTimelineEvents(events: Event[]): Event[] {
	const kept: Event[] = [];
	for (const event of events) {
		if (isBoilerplateEvent(event)) continue;
		if (isLowSignalStandaloneEvent(event)) continue;
		const previous = kept[kept.length - 1];
		if (previous && shouldCoalesceConsecutiveEvents(previous, event)) {
			kept[kept.length - 1] = pickPreferredDuplicate(previous, event);
			continue;
		}
		kept.push(event);
	}
	return kept;
}

function semanticCallId(event: Event): string | null {
	const attrs = event.attributes ?? {};
	const fromSemantic = attrs['semantic.call_id'];
	if (typeof fromSemantic === 'string' && fromSemantic.trim().length > 0) return fromSemantic;
	if (event.event_type.type === 'ToolResult') {
		const fromType = event.event_type.data.call_id;
		if (typeof fromType === 'string' && fromType.trim().length > 0) return fromType;
	}
	return null;
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
