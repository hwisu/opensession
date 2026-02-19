import type { Session } from './types';

type HailHeaderLine = {
	type: 'header';
	version: string;
	session_id: string;
	agent: Session['agent'];
	context: Session['context'];
};

function isRecord(value: unknown): value is Record<string, unknown> {
	return value != null && typeof value === 'object' && !Array.isArray(value);
}

function isSessionLike(value: unknown): value is Session {
	if (!isRecord(value)) return false;
	return (
		typeof value.version === 'string' &&
		typeof value.session_id === 'string' &&
		isRecord(value.agent) &&
		isRecord(value.context) &&
		Array.isArray(value.events) &&
		isRecord(value.stats)
	);
}

function withDefaultStats(events: Session['events']): Session['stats'] {
	return {
		event_count: events.length,
		message_count: 0,
		tool_call_count: 0,
		task_count: 0,
		duration_seconds: 0,
		total_input_tokens: 0,
		total_output_tokens: 0,
	};
}

function parseHailObject(value: unknown): Session {
	if (!isSessionLike(value)) {
		throw new Error('Input is not a valid HAIL session object');
	}

	const session = value as Session;
	return {
		...session,
		stats: session.stats ?? withDefaultStats(session.events),
	};
}

export function parseHailJsonl(text: string): Session {
	const lines = text.split('\n').filter((line) => line.trim().length > 0);
	if (lines.length === 0) throw new Error('Empty JSONL');

	const firstLine = JSON.parse(lines[0]) as Record<string, unknown>;
	if (firstLine.type !== 'header') {
		throw new Error('First line must be a HAIL header');
	}

	const header = firstLine as HailHeaderLine;
	const events: Session['events'] = [];
	let stats: Session['stats'] | null = null;

	for (let i = 1; i < lines.length; i++) {
		const line = JSON.parse(lines[i]) as Record<string, unknown> & { type?: string };
		if (line.type === 'event') {
			const event = { ...line };
			delete event.type;
			events.push(event as unknown as Session['events'][number]);
			continue;
		}
		if (line.type === 'stats') {
			const lineStats = { ...line };
			delete lineStats.type;
			stats = lineStats as unknown as Session['stats'];
		}
	}

	return {
		version: header.version,
		session_id: header.session_id,
		agent: header.agent,
		context: header.context,
		events,
		stats: stats ?? withDefaultStats(events),
	};
}

export function parseHailInput(raw: string): Session {
	const text = raw.trim();
	if (!text) throw new Error('Input is empty');

	try {
		const parsed = JSON.parse(text);
		return parseHailObject(parsed);
	} catch (jsonErr) {
		try {
			return parseHailJsonl(text);
		} catch (jsonlErr) {
			const primary = jsonErr instanceof Error ? jsonErr.message : String(jsonErr);
			const secondary = jsonlErr instanceof Error ? jsonlErr.message : String(jsonlErr);
			throw new Error(`Failed to parse input as JSON or JSONL (${primary}; ${secondary})`);
		}
	}
}
