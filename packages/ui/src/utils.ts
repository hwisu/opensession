import type { Event, Session } from './types';

export interface FileStats {
	filesChanged: number;
	linesAdded: number;
	linesRemoved: number;
}

/** Strip XML-like tags (system-reminder, command-name, task-notification, etc.) */
export function stripTags(text: string): string {
	return text
		.replace(/<[^>]+>/g, '')
		.replace(/\s+/g, ' ')
		.trim();
}

/** Extract a display title from a Session: context.title -> first user message -> fallback */
export function getDisplayTitle(session: Session): string {
	if (session.context.title) {
		const clean = stripTags(session.context.title);
		if (clean) return clean;
	}
	for (const ev of session.events) {
		if (ev.event_type.type === 'UserMessage') {
			for (const block of ev.content.blocks) {
				if (block.type === 'Text' && block.text.trim()) {
					const text = stripTags(block.text.trim());
					if (!text) continue;
					return text.length > 80 ? `${text.slice(0, 77)}...` : text;
				}
			}
		}
	}
	return 'Untitled Session';
}

/** Compute file change statistics from events */
export function computeFileStats(events: Event[]): FileStats {
	const files = new Set<string>();
	let added = 0;
	let removed = 0;
	for (const ev of events) {
		const t = ev.event_type;
		if (t.type === 'FileEdit' || t.type === 'FileCreate') {
			files.add(t.data.path);
			if (t.type === 'FileEdit' && t.data.diff) {
				for (const line of t.data.diff.split('\n')) {
					if (line.startsWith('+') && !line.startsWith('+++')) added++;
					if (line.startsWith('-') && !line.startsWith('---')) removed++;
				}
			}
		} else if (t.type === 'FileDelete') {
			files.add(t.data.path);
		}
	}
	return { filesChanged: files.size, linesAdded: added, linesRemoved: removed };
}

/** Format timestamp as a full localized date string */
export function formatFullDate(ts: string): string {
	const date = new Date(ts);
	return date.toLocaleDateString(undefined, {
		year: 'numeric',
		month: 'short',
		day: 'numeric',
		hour: '2-digit',
		minute: '2-digit',
	});
}
