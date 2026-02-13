import { TRUNCATE_LENGTH } from './constants';
import type { ContentBlock, EventType } from './types';

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
