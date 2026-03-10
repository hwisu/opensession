import type { Content, ContentBlock, Event, EventType, Session, SessionContext } from './types';

const JOB_PROTOCOL_KEY = 'opensession.job.protocol';
const JOB_SYSTEM_KEY = 'opensession.job.system';
const JOB_ID_KEY = 'opensession.job.id';
const JOB_TITLE_KEY = 'opensession.job.title';
const JOB_RUN_ID_KEY = 'opensession.job.run_id';
const JOB_ATTEMPT_KEY = 'opensession.job.attempt';
const JOB_STAGE_KEY = 'opensession.job.stage';
const JOB_REVIEW_KIND_KEY = 'opensession.job.review_kind';
const JOB_STATUS_KEY = 'opensession.job.status';
const JOB_THREAD_ID_KEY = 'opensession.job.thread_id';
const JOB_ARTIFACTS_KEY = 'opensession.job.artifacts';
const REVIEW_NAMESPACE_PREFIX = 'opensession.review.';
const HANDOFF_NAMESPACE_PREFIX = 'opensession.handoff.';
const DIFF_NAMESPACE_PREFIX = 'opensession.diff.';

type LegacyHailHeaderLine = {
	type: 'header';
	version: string;
	session_id: string;
	agent: Session['agent'];
	context: Session['context'];
};

type AcpSessionNewLine = {
	type: 'session.new';
	sessionId: string;
	cwd?: string;
	mcpServers?: unknown[];
	_meta?: Record<string, unknown>;
};

type AcpSessionUpdateLine = {
	type: 'session.update';
	sessionId: string;
	update: Record<string, unknown>;
	_meta?: Record<string, unknown>;
};

type AcpSessionEndLine = {
	type: 'session.end';
	sessionId: string;
	_meta?: Record<string, unknown>;
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

function normalizeContext(context: Session['context']): Session['context'] {
	const tags = Array.isArray(context.tags)
		? context.tags.filter((tag): tag is string => typeof tag === 'string')
		: [];
	const relatedSessionIds = Array.isArray(context.related_session_ids)
		? context.related_session_ids.filter((id): id is string => typeof id === 'string')
		: [];
	const attributes =
		context.attributes != null &&
		typeof context.attributes === 'object' &&
		!Array.isArray(context.attributes)
			? context.attributes
			: {};

	return {
		...context,
		tags,
		related_session_ids: relatedSessionIds,
		attributes,
	};
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
		user_message_count: 0,
		files_changed: 0,
		lines_added: 0,
		lines_removed: 0,
	};
}

function normalizeStats(
	events: Session['events'],
	stats: Session['stats'] | null | undefined,
): Session['stats'] {
	const defaults = withDefaultStats(events);
	if (!stats) return defaults;
	return {
		...defaults,
		...stats,
	};
}

function normalizeAgent(value: unknown): Session['agent'] {
	const agent = isRecord(value) ? value : {};
	return {
		provider: typeof agent.provider === 'string' ? agent.provider : 'unknown',
		model: typeof agent.model === 'string' ? agent.model : 'unknown',
		tool: typeof agent.tool === 'string' ? agent.tool : 'acp',
		tool_version: typeof agent.tool_version === 'string' ? agent.tool_version : undefined,
	};
}

function normalizeStatsRecord(value: unknown): Session['stats'] | null {
	if (!isRecord(value)) return null;
	return {
		event_count: typeof value.event_count === 'number' ? value.event_count : 0,
		message_count: typeof value.message_count === 'number' ? value.message_count : 0,
		tool_call_count: typeof value.tool_call_count === 'number' ? value.tool_call_count : 0,
		task_count: typeof value.task_count === 'number' ? value.task_count : 0,
		duration_seconds: typeof value.duration_seconds === 'number' ? value.duration_seconds : 0,
		total_input_tokens:
			typeof value.total_input_tokens === 'number' ? value.total_input_tokens : 0,
		total_output_tokens:
			typeof value.total_output_tokens === 'number' ? value.total_output_tokens : 0,
		user_message_count:
			typeof value.user_message_count === 'number' ? value.user_message_count : 0,
		files_changed: typeof value.files_changed === 'number' ? value.files_changed : 0,
		lines_added: typeof value.lines_added === 'number' ? value.lines_added : 0,
		lines_removed: typeof value.lines_removed === 'number' ? value.lines_removed : 0,
	};
}

function parseHailObject(value: unknown): Session {
	if (!isSessionLike(value)) {
		throw new Error('Input is not a valid session object');
	}

	const session = value as Session;
	return {
		...session,
		context: normalizeContext(session.context),
		stats: normalizeStats(session.events, session.stats),
	};
}

function getOpenSessionMeta(meta: unknown): Record<string, unknown> | null {
	if (!isRecord(meta)) return null;
	const opensession = meta.opensession;
	return isRecord(opensession) ? opensession : null;
}

function getEventMeta(meta: unknown): Record<string, unknown> | null {
	const opensession = getOpenSessionMeta(meta);
	const event = opensession?.event;
	return isRecord(event) ? event : null;
}

function arrayOfStrings(value: unknown): string[] {
	return Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : [];
}

function copyObject(value: unknown): Record<string, unknown> {
	return isRecord(value) ? { ...value } : {};
}

function restoreJobAttributes(job: unknown, attributes: Record<string, unknown>) {
	if (!isRecord(job)) return;
	if (job.protocol !== undefined) attributes[JOB_PROTOCOL_KEY] = job.protocol;
	if (job.system !== undefined) attributes[JOB_SYSTEM_KEY] = job.system;
	if (job.jobId !== undefined) attributes[JOB_ID_KEY] = job.jobId;
	if (job.jobTitle !== undefined) attributes[JOB_TITLE_KEY] = job.jobTitle;
	if (job.runId !== undefined) attributes[JOB_RUN_ID_KEY] = job.runId;
	if (job.attempt !== undefined) attributes[JOB_ATTEMPT_KEY] = job.attempt;
	if (job.stage !== undefined) attributes[JOB_STAGE_KEY] = job.stage;
	if (job.reviewKind !== undefined) attributes[JOB_REVIEW_KIND_KEY] = job.reviewKind;
	if (job.status !== undefined) attributes[JOB_STATUS_KEY] = job.status;
	if (job.threadId !== undefined) attributes[JOB_THREAD_ID_KEY] = job.threadId;
	if (job.artifacts !== undefined) attributes[JOB_ARTIFACTS_KEY] = job.artifacts;
}

function restorePrefixedNamespace(
	value: unknown,
	prefix: string,
	attributes: Record<string, unknown>,
) {
	const flatten = (current: unknown, path: string[]) => {
		if (!isRecord(current)) {
			if (path.length > 0) {
				attributes[`${prefix}${path.join('.')}`] = current;
			}
			return;
		}

		for (const [key, child] of Object.entries(current)) {
			flatten(child, [...path, key]);
		}
	};

	flatten(value, []);
}

function parseAcpContext(line: AcpSessionNewLine): Session {
	const opensession = getOpenSessionMeta(line._meta);
	const contextMeta = isRecord(opensession?.context) ? opensession.context : {};
	const attributes = copyObject(contextMeta.attributes);
	if (typeof line.cwd === 'string' && line.cwd.length > 0) {
		attributes.cwd = line.cwd;
	}
	if (Array.isArray(line.mcpServers) && line.mcpServers.length > 0) {
		attributes.mcp_servers = line.mcpServers;
	}
	restoreJobAttributes(opensession?.job, attributes);
	restorePrefixedNamespace(opensession?.review, REVIEW_NAMESPACE_PREFIX, attributes);
	restorePrefixedNamespace(opensession?.handoff, HANDOFF_NAMESPACE_PREFIX, attributes);

	const source = isRecord(opensession?.source) ? opensession.source : {};
	const version =
		typeof source.sessionVersion === 'string' && source.sessionVersion.length > 0
			? source.sessionVersion
			: 'acp-semantic-1.0.0';

	const context: SessionContext = normalizeContext({
		title: typeof contextMeta.title === 'string' ? contextMeta.title : undefined,
		description: typeof contextMeta.description === 'string' ? contextMeta.description : undefined,
		tags: arrayOfStrings(contextMeta.tags),
		created_at:
			typeof contextMeta.createdAt === 'string'
				? contextMeta.createdAt
				: new Date().toISOString(),
		updated_at:
			typeof contextMeta.updatedAt === 'string'
				? contextMeta.updatedAt
				: new Date().toISOString(),
		related_session_ids: arrayOfStrings(contextMeta.relatedSessionIds),
		attributes,
	});

	return {
		version,
		session_id: line.sessionId,
		agent: normalizeAgent(opensession?.agent),
		context,
		events: [],
		stats: withDefaultStats([]),
	};
}

function parseAcpContentBlock(value: unknown): ContentBlock {
	if (!isRecord(value)) {
		return { type: 'Text', text: String(value ?? '') };
	}
	if (value.type === 'resource_link') {
		return {
			type: 'Reference',
			uri: typeof value.uri === 'string' ? value.uri : '',
			media_type:
				typeof value.mimeType === 'string' && value.mimeType.length > 0
					? value.mimeType
					: 'application/octet-stream',
		};
	}
	return {
		type: 'Text',
		text: typeof value.text === 'string' ? value.text : JSON.stringify(value),
	};
}

function fallbackContentFromUpdate(update: Record<string, unknown>): Content {
	const sessionUpdate = update.sessionUpdate;
	if (
		sessionUpdate === 'user_message_chunk' ||
		sessionUpdate === 'agent_message_chunk' ||
		sessionUpdate === 'system_message_chunk' ||
		sessionUpdate === 'agent_thought_chunk'
	) {
		return { blocks: [parseAcpContentBlock(update.content)] };
	}
	if (sessionUpdate === 'tool_call' || sessionUpdate === 'tool_call_update') {
		const content = Array.isArray(update.content) ? update.content : [];
		return {
			blocks: content.map((item) => {
				if (isRecord(item) && item.type === 'content') {
					return parseAcpContentBlock(item.content);
				}
				if (isRecord(item) && item.type === 'diff') {
					return {
						type: 'Json',
						data: {
							path: item.path,
							oldText: item.oldText,
							newText: item.newText,
						},
					} satisfies ContentBlock;
				}
				return { type: 'Text', text: JSON.stringify(item) } satisfies ContentBlock;
			}),
		};
	}
	return { blocks: [] };
}

function deriveEventType(update: Record<string, unknown>): EventType {
	switch (update.sessionUpdate) {
		case 'user_message_chunk':
			return { type: 'UserMessage' };
		case 'agent_message_chunk':
			return { type: 'AgentMessage' };
		case 'system_message_chunk':
			return { type: 'SystemMessage' };
		case 'agent_thought_chunk':
			return { type: 'Thinking' };
		case 'tool_call':
			return {
				type: 'ToolCall',
				data: { name: typeof update.title === 'string' ? update.title : 'tool_call' },
			};
		case 'tool_call_update':
			return {
				type: 'ToolResult',
				data: {
					name: 'tool_call',
					is_error: update.status === 'failed',
					call_id: typeof update.toolCallId === 'string' ? update.toolCallId : undefined,
				},
			};
		default:
			return { type: 'Custom', data: { kind: String(update.sessionUpdate ?? 'acp_update') } };
	}
}

type MutableEvent = {
	event_id: string;
	timestamp: string;
	event_type: EventType;
	task_id?: string;
	content: Content;
	duration_ms?: number;
	attributes: Record<string, unknown>;
};

function cloneContent(value: unknown): Content | null {
	if (!isRecord(value) || !Array.isArray(value.blocks)) return null;
	return { blocks: value.blocks as ContentBlock[] };
}

function parseAcpJsonl(text: string): Session {
	const lines = text
		.split('\n')
		.filter((line) => line.trim().length > 0)
		.map((line) => JSON.parse(line) as LegacyHailHeaderLine | AcpSessionNewLine | AcpSessionUpdateLine | AcpSessionEndLine);
	if (lines.length === 0) throw new Error('Empty JSONL');
	if (lines[0].type !== 'session.new') throw new Error('First ACP line must be session.new');

	const session = parseAcpContext(lines[0] as AcpSessionNewLine);
	const events: MutableEvent[] = [];
	let current: MutableEvent | null = null;
	let stats: Session['stats'] | null = null;

	for (const line of lines.slice(1)) {
		if (line.type === 'session.update') {
			const updateLine = line as AcpSessionUpdateLine;
			const opensession = getOpenSessionMeta(updateLine._meta);
			const eventMeta = getEventMeta(updateLine._meta);
			const eventId =
				typeof eventMeta?.eventId === 'string' && eventMeta.eventId.length > 0
					? eventMeta.eventId
					: `event-${events.length + 1}`;

			if (!current || current.event_id !== eventId) {
				if (current) events.push(current);
				current = {
					event_id: eventId,
					timestamp:
						typeof eventMeta?.timestamp === 'string'
							? eventMeta.timestamp
							: session.context.created_at,
					event_type: isRecord(eventMeta?.originalEventType)
						? (eventMeta.originalEventType as EventType)
						: deriveEventType(updateLine.update),
					task_id:
						typeof eventMeta?.taskId === 'string' ? eventMeta.taskId : undefined,
					content:
						cloneContent(eventMeta?.originalContent) ?? fallbackContentFromUpdate(updateLine.update),
					duration_ms:
						typeof eventMeta?.durationMs === 'number' ? eventMeta.durationMs : undefined,
					attributes: copyObject(eventMeta?.attributes),
				};
				restorePrefixedNamespace(opensession?.diff, DIFF_NAMESPACE_PREFIX, current.attributes);
			} else if (!cloneContent(eventMeta?.originalContent)) {
				current.content.blocks.push(...fallbackContentFromUpdate(updateLine.update).blocks);
			}
			continue;
		}

		if (line.type === 'session.end') {
			const opensession = getOpenSessionMeta((line as AcpSessionEndLine)._meta);
			stats = normalizeStatsRecord(opensession?.stats);
		}
	}

	if (current) events.push(current);

	return {
		...session,
		events: events as Event[],
		stats: normalizeStats(events as Event[], stats),
	};
}

export function parseHailJsonl(text: string): Session {
	const lines = text.split('\n').filter((line) => line.trim().length > 0);
	if (lines.length === 0) throw new Error('Empty JSONL');

	const firstLine = JSON.parse(lines[0]) as Record<string, unknown>;
	if (firstLine.type === 'session.new') {
		return parseAcpJsonl(text);
	}
	if (firstLine.type !== 'header') {
		throw new Error('First line must be a HAIL header or ACP session.new');
	}

	const header = firstLine as LegacyHailHeaderLine;
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
		context: normalizeContext(header.context),
		events,
		stats: normalizeStats(events, stats),
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
