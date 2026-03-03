<script lang="ts">
import { CHIP_LABEL_MAX } from '../constants';
import {
	calcContentLength,
	findCodeStats,
	findFirstText,
	firstMeaningfulEventLine,
	formatContentLength,
	getToolName,
	isToolError,
	truncate,
} from '../event-helpers';
import { highlightCode } from '../highlight';
import { isLongContent, renderMarkdown } from '../markdown';
import type { Event } from '../types';
import CodeBlockView from './CodeBlockView.svelte';
import ContentBlockList from './ContentBlockList.svelte';
import DiffView from './DiffView.svelte';
import ExpandableChip from './ExpandableChip.svelte';
import {
	arrowLeftIcon,
	arrowRightIcon,
	chevronRightIcon,
	fileEditIcon,
	fileIcon,
	globeIcon,
	imageIcon,
	lightningIcon,
	searchIcon,
	taskEndIcon,
	taskStartIcon,
	terminalIcon,
	trashIcon,
} from './icons';

const {
	event,
	pairedResult,
	resultOk = false,
}: { event: Event; pairedResult?: Event; resultOk?: boolean } = $props();

// --- Helpers ---
type RequestUserInputOption = {
	label: string;
	description: string | null;
};

type RequestUserInputQuestion = {
	id: string;
	header: string | null;
	question: string | null;
	options: RequestUserInputOption[];
};

type RequestUserInputCallPayload = {
	questions: RequestUserInputQuestion[];
};

type RequestUserInputAnswer = {
	id: string;
	answers: string[];
	raw: string | null;
};

type RequestUserInputResultPayload = {
	answers: RequestUserInputAnswer[];
};

function asObject(value: unknown): Record<string, unknown> | null {
	if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
	return value as Record<string, unknown>;
}

function asNonEmptyString(value: unknown): string | null {
	if (typeof value !== 'string') return null;
	const trimmed = value.trim();
	return trimmed.length > 0 ? trimmed : null;
}

function parseJsonText(text: string): unknown | null {
	const trimmed = text.trim();
	if (trimmed.length === 0) return null;
	const first = trimmed[0];
	if (first !== '{' && first !== '[') return null;
	try {
		return JSON.parse(trimmed);
	} catch {
		return null;
	}
}

function firstStructuredPayload(event: Event): unknown | null {
	for (const block of event.content.blocks) {
		if (block.type === 'Json') return block.data;
		if (block.type === 'Text') {
			const parsed = parseJsonText(block.text);
			if (parsed != null) return parsed;
		}
	}
	return null;
}

function answerValues(value: unknown): { answers: string[]; raw: string | null } {
	if (value == null) return { answers: [], raw: null };
	const valueObject = asObject(value);
	const options = valueObject?.answers;
	if (Array.isArray(options)) {
		const answers = options
			.map((item) => {
				const direct = asNonEmptyString(item);
				if (direct) return direct;
				const fromObject = asObject(item)?.value;
				const fromValue = asNonEmptyString(fromObject);
				if (fromValue) return fromValue;
				if (typeof item === 'number' || typeof item === 'boolean') return String(item);
				return null;
			})
			.filter((entry): entry is string => entry != null);
		if (answers.length > 0) return { answers, raw: null };
	}
	const direct = asNonEmptyString(value);
	if (direct) return { answers: [direct], raw: null };
	if (typeof value === 'number' || typeof value === 'boolean') {
		return { answers: [String(value)], raw: null };
	}
	try {
		return { answers: [], raw: JSON.stringify(value) };
	} catch {
		return { answers: [], raw: String(value) };
	}
}

function parseRequestUserInputOptions(value: unknown): RequestUserInputOption[] {
	if (!Array.isArray(value)) return [];
	return value
		.map((entry) => {
			const direct = asNonEmptyString(entry);
			if (direct) {
				return {
					label: direct,
					description: null,
				};
			}
			const row = asObject(entry);
			if (!row) return null;
			const label =
				asNonEmptyString(row.label) ??
				asNonEmptyString(row.title) ??
				asNonEmptyString(row.text) ??
				asNonEmptyString(row.value);
			if (!label) return null;
			return {
				label,
				description: asNonEmptyString(row.description),
			};
		})
		.filter((option): option is RequestUserInputOption => option != null);
}

function parseRequestUserInputCallPayload(payload: unknown): RequestUserInputCallPayload | null {
	const root = asObject(payload);
	if (!root) return null;
	const items = root.questions;
	const questions = Array.isArray(items)
		? items
				.map((entry) => {
					const item = asObject(entry);
					if (!item) return null;
					const id = asNonEmptyString(item.id) ?? 'question';
					const header = asNonEmptyString(item.header);
					const question =
						asNonEmptyString(item.question) ??
						asNonEmptyString(item.prompt) ??
						asNonEmptyString(item.ask);
					const options = parseRequestUserInputOptions(item.options ?? item.choices);
					return {
						id,
						header,
						question,
						options,
					};
				})
				.filter((question): question is RequestUserInputQuestion => question != null)
		: [];

	if (questions.length === 0) {
		const fallbackQuestion =
			asNonEmptyString(root.question) ?? asNonEmptyString(root.prompt) ?? asNonEmptyString(root.ask);
		if (!fallbackQuestion) return null;
		return {
			questions: [
				{
					id: asNonEmptyString(root.id) ?? 'question',
					header: asNonEmptyString(root.header) ?? asNonEmptyString(root.title),
					question: fallbackQuestion,
					options: parseRequestUserInputOptions(root.options ?? root.choices),
				},
			],
		};
	}

	return { questions };
}

function parseRequestUserInputResultPayload(payload: unknown): RequestUserInputResultPayload | null {
	const root = asObject(payload);
	if (!root) return null;
	const answerMap = asObject(root.answers);
	const answers = answerMap
		? Object.entries(answerMap).map(([id, value]) => {
				const normalized = answerValues(value);
				return {
					id,
					answers: normalized.answers,
					raw: normalized.raw,
				};
			})
		: [];

	if (answers.length === 0) {
		const fallbackAnswer =
			answerValues(root.answer).answers[0] ??
			answerValues(root.response).answers[0] ??
			answerValues(root.user_message).answers[0];
		if (!fallbackAnswer) return null;
		return {
			answers: [
				{
					id: asNonEmptyString(root.id) ?? 'question',
					answers: [fallbackAnswer],
					raw: null,
				},
			],
		};
	}

	return { answers };
}

function shortPath(path: string): string {
	const parts = path.split('/');
	return parts.length > 3 ? `.../${parts.slice(-3).join('/')}` : path;
}

function diffStats(diff: string): { added: number; removed: number; modified: number } {
	let added = 0,
		removed = 0,
		modified = 0;
	for (const line of diff.split('\n')) {
		if (line.startsWith('+') && !line.startsWith('+++')) added++;
		else if (line.startsWith('-') && !line.startsWith('---')) removed++;
	}
	modified = Math.min(added, removed);
	added -= modified;
	removed -= modified;
	return { added, removed, modified };
}

// --- Classify ---
const eventTypeName = $derived(event.event_type.type);
const isMessage = $derived(
	['UserMessage', 'AgentMessage', 'SystemMessage'].includes(eventTypeName),
);
const isThinking = $derived(eventTypeName === 'Thinking');
const isSubAgent = $derived(
	eventTypeName === 'ToolCall' &&
		'data' in event.event_type &&
		(event.event_type.data as Record<string, unknown>)?.name === 'Task',
);

// --- State (initType captures once — event type is immutable) ---
// svelte-ignore state_referenced_locally
const initType = event.event_type.type;
let expanded = $state(['UserMessage', 'AgentMessage', 'SystemMessage'].includes(initType));
let showFull = $state(false);

// --- Derived ---
const hasContent = $derived(event.content.blocks.length > 0);

const isError = $derived(isToolError(event.event_type));
const contentLength = $derived(calcContentLength(event.content.blocks));
const codeStats = $derived(findCodeStats(event.content.blocks));

const subAgentDesc = $derived.by(() => {
	if (!isSubAgent) return null;
	return findFirstText(event.content.blocks);
});

// --- Icon lookup table ---
const ICON_MAP: Record<string, string> = {
	FileRead: fileIcon,
	FileEdit: fileEditIcon,
	FileCreate: fileIcon,
	FileDelete: trashIcon,
	ShellCommand: terminalIcon,
	WebSearch: searchIcon,
	FileSearch: searchIcon,
	CodeSearch: searchIcon,
	WebFetch: globeIcon,
	ImageGenerate: imageIcon,
	TaskStart: taskStartIcon,
	TaskEnd: taskEndIcon,
};

const chipIcon = $derived(
	eventTypeName === 'ToolCall'
		? lightningIcon
		: eventTypeName === 'ToolResult'
			? arrowLeftIcon
			: (ICON_MAP[eventTypeName] ?? '&#x2022;'),
);

const toolEventName = $derived.by(() => {
	if (event.event_type.type !== 'ToolCall' && event.event_type.type !== 'ToolResult') return null;
	return getToolName(event.event_type).toLowerCase();
});

const isRequestUserInputTool = $derived(toolEventName === 'request_user_input');

const requestUserInputCallPayload = $derived.by(() => {
	if (!isRequestUserInputTool || event.event_type.type !== 'ToolCall') return null;
	return parseRequestUserInputCallPayload(firstStructuredPayload(event));
});

const requestUserInputResultPayload = $derived.by(() => {
	if (!isRequestUserInputTool || event.event_type.type !== 'ToolResult') return null;
	return parseRequestUserInputResultPayload(firstStructuredPayload(event));
});

const pairedRequestUserInputResultPayload = $derived.by(() => {
	if (!pairedResult) return null;
	if (pairedResult.event_type.type !== 'ToolResult') return null;
	if (getToolName(pairedResult.event_type).toLowerCase() !== 'request_user_input') return null;
	return parseRequestUserInputResultPayload(firstStructuredPayload(pairedResult));
});

// --- Chip label ---
const summaryLabel = $derived.by(() => {
	const t = event.event_type;
	switch (t.type) {
		case 'FileRead':
		case 'FileEdit':
		case 'FileCreate':
		case 'FileDelete':
			return shortPath(t.data.path);
		case 'ShellCommand':
			return truncate(t.data.command, CHIP_LABEL_MAX);
		case 'WebSearch':
			return t.data.query;
		case 'WebFetch': {
			try {
				const u = new URL(t.data.url);
				return u.hostname + u.pathname;
			} catch {
				return t.data.url;
			}
		}
		case 'CodeSearch':
			return t.data.query;
			case 'FileSearch':
				return t.data.pattern;
			case 'ToolCall': {
				if (isRequestUserInputTool && requestUserInputCallPayload) {
					const first = requestUserInputCallPayload.questions[0];
					const label = first?.header ?? first?.question ?? first?.id ?? 'request_user_input';
					if (requestUserInputCallPayload.questions.length === 1) return label;
					return `${label} +${requestUserInputCallPayload.questions.length - 1}`;
				}
				for (const block of event.content.blocks) {
					if (block.type === 'Json' && block.data && typeof block.data === 'object') {
						const d = block.data as Record<string, unknown>;
						if (d.subject) return String(d.subject);
					if (d.recipient) return `\u2192 ${String(d.recipient)}`;
					if (d.taskId && d.status) return `#${d.taskId} \u2192 ${d.status}`;
				}
				if (block.type === 'Text' && block.text.trim()) {
					return truncate(block.text.trim());
				}
			}
			return '';
			}
			case 'ToolResult': {
				if (isRequestUserInputTool && requestUserInputResultPayload) {
					if (requestUserInputResultPayload.answers.length === 0) return 'no answers';
					const first = requestUserInputResultPayload.answers[0];
					const preview = first.answers[0] ?? first.raw ?? '(no answer)';
					if (requestUserInputResultPayload.answers.length === 1) return `${first.id}: ${preview}`;
					return `${first.id}: ${preview} +${requestUserInputResultPayload.answers.length - 1}`;
				}
				const line = firstMeaningfulEventLine(event);
				return line ? truncate(line) : 'output';
			}
		case 'TaskStart':
			return t.data.title ?? '';
		case 'TaskEnd':
			return t.data.summary ?? '';
		case 'ImageGenerate':
			return t.data.prompt;
		default:
			return '';
	}
});

// --- Chip meta ---
const metaBadgeText = $derived.by(() => {
	const t = event.event_type;
	if (t.type === 'FileRead' && codeStats) {
		return `L1-${codeStats.lines}`;
	}
	if (t.type === 'FileEdit' && t.data.diff) {
		const s = diffStats(t.data.diff);
		const parts: string[] = [];
		if (s.added > 0) parts.push(`+${s.added}`);
		if (s.removed > 0) parts.push(`-${s.removed}`);
		if (s.modified > 0) parts.push(`~${s.modified}`);
		return parts.join(' ');
	}
	if (t.type === 'FileCreate') {
		const lines = event.content.blocks.reduce(
			(n, b) => n + (b.type === 'Code' ? b.code.split('\n').length : 0),
			0,
		);
		return lines > 0 ? `+${lines}` : '';
	}
	if (t.type === 'FileSearch' || t.type === 'CodeSearch') {
		const count = event.content.blocks.reduce(
			(n, b) => n + (b.type === 'Text' ? b.text.split('\n').filter(Boolean).length : 0),
			0,
		);
		return count > 0 ? `${count} ${t.type === 'FileSearch' ? 'files' : 'matches'}` : '';
	}
	return null;
});

// --- Chip name label (for ToolCall/ToolResult) ---
const chipNameLabel = $derived(
	eventTypeName === 'ToolCall' || eventTypeName === 'ToolResult'
		? getToolName(event.event_type)
		: '',
);

const pairedResultIsError = $derived(pairedResult ? isToolError(pairedResult.event_type) : false);

const chipNameColorClass = $derived(
	isError
		? 'text-error'
		: eventTypeName === 'ToolResult'
			? 'text-success'
			: pairedResult
				? pairedResultIsError
					? 'text-error'
					: 'text-text-muted'
				: 'text-text-muted',
);

const hasCodeBlock = $derived(event.content.blocks.some((b) => b.type === 'Code'));
</script>

<!-- ═══ MESSAGE (User / Agent / System) ═══ -->
{#if isMessage}
	{@const isUser = eventTypeName === 'UserMessage'}
	{@const isSystem = eventTypeName === 'SystemMessage'}
	<div class="ev-message group my-1 sm:my-4" data-event-type={eventTypeName}>
		{#if isUser}
			<div class="flex items-start gap-1.5 sm:gap-3">
				<span class="tui-badge tui-badge-user mt-0.5 shrink-0">USER</span>
				{#if event.timestamp}
					<span class="mt-0.5 shrink-0 text-[10px] text-text-muted hidden sm:inline">{new Date(event.timestamp).toLocaleTimeString('en-US', { hour12: false })}</span>
				{/if}
				<div class="min-w-0 flex-1">
					<div class="border-l-2 border-l-green-400/30 pl-2 sm:pl-3 text-sm leading-relaxed">
						<ContentBlockList blocks={event.content.blocks} bind:showFull />
					</div>
				</div>
			</div>
		{:else if isSystem}
			<div class="flex items-start gap-1.5 sm:gap-3">
				<span class="tui-badge tui-badge-system mt-0.5 shrink-0">SYSTEM</span>
				{#if event.timestamp}
					<span class="mt-0.5 shrink-0 text-[10px] text-text-muted hidden sm:inline">{new Date(event.timestamp).toLocaleTimeString('en-US', { hour12: false })}</span>
				{/if}
				<div class="min-w-0 flex-1 text-sm leading-relaxed">
					<ContentBlockList blocks={event.content.blocks} bind:showFull showJson={true} />
				</div>
			</div>
		{:else}
			<div class="flex items-start gap-1.5 sm:gap-3">
				<span class="tui-badge tui-badge-agent mt-0.5 shrink-0">AGENT</span>
				{#if event.timestamp}
					<span class="mt-0.5 shrink-0 text-[10px] text-text-muted hidden sm:inline">{new Date(event.timestamp).toLocaleTimeString('en-US', { hour12: false })}</span>
				{/if}
				<div class="min-w-0 flex-1 text-sm leading-relaxed">
					<ContentBlockList blocks={event.content.blocks} bind:showFull showJson={true} />
				</div>
			</div>
		{/if}
	</div>

<!-- ═══ THINKING ═══ -->
{:else if isThinking}
	<div class="ev-thinking my-0.5" data-event-type="Thinking">
		<button
			onclick={() => (expanded = !expanded)}
			class="group flex w-full items-center gap-1.5 px-2 py-1.5 text-left text-xs transition-colors hover:bg-bg-hover"
		>
			<span class="inline-flex text-text-muted transition-transform" class:rotate-90={expanded}>{@html chevronRightIcon}</span>
			<span class="font-medium text-text-muted group-hover:text-text-secondary">Thinking</span>
			{#if event.duration_ms}
				<span class="ml-auto shrink-0 font-mono text-[10px] text-text-muted">{event.duration_ms}ms</span>
			{/if}
		</button>

		{#if expanded && hasContent}
			<div class="ml-4 mt-1 border-l border-border pl-3 text-xs">
				{#each event.content.blocks as block}
					{#if block.type === 'Text'}
						{@const long = isLongContent(block.text)}
						{#if block.text.trim()}
							<div
								class="whitespace-pre-wrap break-words leading-relaxed text-text-muted"
								class:ev-collapsed={long && !showFull}
							>
								{block.text}
							</div>
							{#if long}
								<button
									onclick={() => (showFull = !showFull)}
									class="mt-1 text-[10px] font-medium text-accent hover:underline"
								>
									{showFull ? 'Show less' : 'Show more...'}
								</button>
							{/if}
						{/if}
					{/if}
				{/each}
			</div>
		{/if}
	</div>

<!-- ═══ SUB-AGENT (Task tool call) ═══ -->
{:else if isSubAgent}
	<div class="ev-subagent my-1.5" data-event-type="ToolCall">
		<button
			onclick={() => (expanded = !expanded)}
			class="group flex w-full items-center gap-2 border border-accent/30 border-l-4 border-l-accent bg-accent/10 px-3 py-1.5 text-left text-xs transition-colors hover:bg-bg-hover"
		>
			<span class="inline-flex text-accent">{@html arrowRightIcon}</span>
			<span class="flex-1 font-medium text-text-secondary truncate">{subAgentDesc ? truncate(subAgentDesc, CHIP_LABEL_MAX) : 'Sub-agent'}</span>
			{#if event.duration_ms}
				<span class="font-mono text-[10px] text-text-muted">{event.duration_ms}ms</span>
			{/if}
			<span class="shrink-0 inline-flex text-text-muted transition-transform" class:rotate-90={expanded}>{@html chevronRightIcon}</span>
		</button>

		{#if expanded}
			<div class="ml-4 mt-1 border-l border-border pl-3">
				{#if subAgentDesc}
					<div class="md-content text-sm text-text-secondary">
						{@html renderMarkdown(subAgentDesc)}
					</div>
				{/if}
				{#each event.content.blocks as block}
					{#if block.type === 'Code'}
						<div class="mt-2">
							<CodeBlockView code={block.code} language={block.language}
								startLine={block.start_line ?? 1} bind:showFull />
						</div>
					{/if}
				{/each}
			</div>
		{/if}
	</div>

<!-- ═══ CHIP: All other event types ═══ -->
{:else}
	{@const isFileEdit = eventTypeName === 'FileEdit'}
	{@const isFileDelete = eventTypeName === 'FileDelete'}
	{@const isShellCommand = eventTypeName === 'ShellCommand'}

	{#if isFileDelete}
		<!-- FileDelete: non-expandable -->
		<div class="ev-chip my-0.5" data-event-type="FileDelete">
			<div class="flex items-center gap-2 border border-transparent bg-transparent px-3 py-1.5 text-xs hover:bg-bg-hover">
				<span class="shrink-0 inline-flex text-text-muted">{@html trashIcon}</span>
				<span class="truncate font-mono text-text-secondary">{summaryLabel}</span>
			</div>
		</div>
	{:else}
		<ExpandableChip
			icon={chipIcon}
			label={summaryLabel}
			bind:expanded
			hasContent={hasContent}
			nameLabel={chipNameLabel}
			nameColorClass={chipNameColorClass}
		>
			{#snippet metaBadge()}
				{#if isError}
					<span class="shrink-0 rounded bg-error/20 px-1.5 py-0.5 text-[10px] text-error">error</span>
				{:else if pairedResult && pairedResultIsError}
					<span class="shrink-0 rounded bg-error/20 px-1.5 py-0.5 text-[10px] text-error">error</span>
				{:else if pairedResult}
					<span class="shrink-0 font-mono text-[10px] text-success">✓</span>
				{:else if resultOk}
					<span class="shrink-0 font-mono text-[10px] text-success">✓</span>
				{:else if isFileEdit && metaBadgeText}
					{@const stats = diffStats(('data' in event.event_type ? (event.event_type.data as { diff?: string }).diff : '') ?? '')}
					<span class="shrink-0 font-mono text-[10px]">
						{#if stats.added > 0}<span class="text-success">+{stats.added + stats.modified}</span>{/if}
						{#if stats.removed > 0}<span class="text-error ml-0.5">-{stats.removed + stats.modified}</span>{/if}
					</span>
				{:else if eventTypeName === 'FileCreate' && metaBadgeText}
					<span class="shrink-0 font-mono text-[10px] text-success">{metaBadgeText}</span>
				{:else if metaBadgeText}
					<span class="shrink-0 font-mono text-[10px] text-text-muted">{metaBadgeText}</span>
				{/if}
				{#if isShellCommand && 'data' in event.event_type && (event.event_type.data as { exit_code?: number }).exit_code != null && (event.event_type.data as { exit_code?: number }).exit_code !== 0}
					<span class="shrink-0 font-mono text-[10px] text-error">↩ {(event.event_type.data as { exit_code?: number }).exit_code}</span>
				{/if}
				{#if !metaBadgeText && hasContent && contentLength > 0 && (isShellCommand || eventTypeName === 'WebFetch' || eventTypeName === 'ToolCall' || eventTypeName === 'ToolResult')}
					<span class="shrink-0 font-mono text-[10px] text-text-muted">{formatContentLength(contentLength)} chars</span>
				{/if}
				{#if eventTypeName === 'ToolResult' && hasCodeBlock && codeStats}
					<span class="shrink-0 font-mono text-[10px] text-text-muted">{codeStats.lines} lines</span>
				{/if}
			{/snippet}

			{#snippet children()}
				{#if requestUserInputCallPayload}
					<div class="space-y-2 p-3">
						{#each requestUserInputCallPayload.questions as question}
							<div class="rounded border border-border/70 bg-bg-secondary/45 px-2.5 py-2">
								<div class="flex flex-wrap items-center gap-1.5 text-[11px] text-text-muted">
									<span class="rounded border border-border bg-bg-primary px-1.5 py-0.5 font-mono text-text-secondary">
										{question.id}
									</span>
									{#if question.header}
										<span class="rounded border border-border bg-bg-primary px-1.5 py-0.5">
											{question.header}
										</span>
									{/if}
								</div>
								{#if question.question}
									<div class="mt-1.5 whitespace-pre-wrap text-xs text-text-primary">
										{question.question}
									</div>
								{/if}
								{#if question.options.length > 0}
									<div class="mt-2 text-[10px] font-medium uppercase tracking-wider text-text-muted">
										Options
									</div>
									<div class="mt-1 space-y-1.5">
										{#each question.options as option, optionIndex}
											<div class="rounded border border-border/60 bg-bg-primary/40 px-2 py-1.5">
												<div class="text-xs text-text-secondary">
													{optionIndex + 1}. {option.label}
												</div>
												{#if option.description}
													<div class="mt-0.5 text-[11px] text-text-muted">
														{option.description}
													</div>
												{/if}
											</div>
										{/each}
									</div>
								{/if}
							</div>
						{/each}
					</div>
				{:else if requestUserInputResultPayload}
					<div class="space-y-2 p-3">
						{#if requestUserInputResultPayload.answers.length === 0}
							<div class="rounded border border-warning/40 bg-warning/10 px-2 py-1.5 text-xs text-warning">
								No answers found in payload.
							</div>
						{:else}
							{#each requestUserInputResultPayload.answers as answer}
								<div class="rounded border border-border/70 bg-bg-secondary/45 px-2.5 py-2">
									<div class="text-[11px] font-mono text-text-secondary">{answer.id}</div>
									{#if answer.answers.length > 0}
										<div class="mt-1.5 flex flex-wrap gap-1">
											{#each answer.answers as choice}
												<span class="rounded border border-accent/35 bg-accent/10 px-1.5 py-0.5 text-[11px] text-accent">
													{choice}
												</span>
											{/each}
										</div>
									{:else if answer.raw}
										<div class="mt-1.5 whitespace-pre-wrap text-xs text-text-muted">{answer.raw}</div>
									{:else}
										<div class="mt-1.5 text-xs text-warning">(no answer)</div>
									{/if}
								</div>
							{/each}
						{/if}
					</div>
				{:else}
					{#each event.content.blocks as block}
						{#if block.type === 'Code'}
							{@const fileEditData = isFileEdit && 'data' in event.event_type ? (event.event_type as { data: { diff?: string } }).data : null}
							{#if fileEditData?.diff}
								<DiffView diff={fileEditData.diff} />
							{:else}
								<CodeBlockView code={block.code} language={block.language}
									startLine={block.start_line ?? 1} bind:showFull />
							{/if}
						{:else if block.type === 'Text'}
							{@const long = isLongContent(block.text)}
							{#if block.text.trim()}
								{@const isPlainText = eventTypeName === 'FileRead' || isShellCommand || eventTypeName === 'FileSearch' || eventTypeName === 'CodeSearch'}
								{#if isPlainText}
									<div class="p-3 text-text-secondary whitespace-pre-wrap" class:font-mono={isShellCommand} class:ev-collapsed={long && !showFull}>
										{block.text}
									</div>
								{:else}
									<div class="md-content p-3 text-text-secondary" class:ev-collapsed={long && !showFull}>
										{@html renderMarkdown(block.text)}
									</div>
								{/if}
								{#if long}
									<button
										onclick={() => (showFull = !showFull)}
										class="w-full border-t border-border bg-bg-secondary px-3 py-1.5 text-center text-[10px] font-medium text-accent hover:bg-bg-hover"
									>
										{showFull ? 'Collapse' : 'Show more...'}
									</button>
								{/if}
							{/if}
						{:else if block.type === 'Json'}
							<pre class="overflow-x-auto p-3 leading-relaxed"><code class="hljs">{@html highlightCode(JSON.stringify(block.data, null, 2), 'json')}</code></pre>
						{:else if block.type === 'Image'}
							<img src={block.url} alt={block.alt ?? ''} class="max-h-64 p-2" />
						{:else if block.type === 'File'}
							<div class="p-3 font-mono text-text-muted">{block.path}</div>
						{/if}
					{/each}
				{/if}
				{#if pairedResult && pairedResult.content.blocks.length > 0}
					<div class="border-t border-border/50 mt-1">
						<div class="px-3 py-1 text-[10px] font-medium text-text-muted uppercase tracking-wider bg-bg-secondary/50">Result</div>
						{#if pairedRequestUserInputResultPayload}
							<div class="space-y-2 p-3">
								{#if pairedRequestUserInputResultPayload.answers.length === 0}
									<div class="rounded border border-warning/40 bg-warning/10 px-2 py-1.5 text-xs text-warning">
										No answers found in payload.
									</div>
								{:else}
									{#each pairedRequestUserInputResultPayload.answers as answer}
										<div class="rounded border border-border/70 bg-bg-secondary/45 px-2.5 py-2">
											<div class="text-[11px] font-mono text-text-secondary">{answer.id}</div>
											{#if answer.answers.length > 0}
												<div class="mt-1.5 flex flex-wrap gap-1">
													{#each answer.answers as choice}
														<span class="rounded border border-accent/35 bg-accent/10 px-1.5 py-0.5 text-[11px] text-accent">
															{choice}
														</span>
													{/each}
												</div>
											{:else if answer.raw}
												<div class="mt-1.5 whitespace-pre-wrap text-xs text-text-muted">{answer.raw}</div>
											{:else}
												<div class="mt-1.5 text-xs text-warning">(no answer)</div>
											{/if}
										</div>
									{/each}
								{/if}
							</div>
						{:else}
							{#each pairedResult.content.blocks as block}
								{#if block.type === 'Code'}
									<CodeBlockView code={block.code} language={block.language}
										startLine={block.start_line ?? 1} bind:showFull />
								{:else if block.type === 'Text'}
									{@const long = isLongContent(block.text)}
									{#if block.text.trim()}
										<div class="p-3 text-text-secondary whitespace-pre-wrap" class:ev-collapsed={long && !showFull}>
											{block.text}
										</div>
										{#if long}
											<button
												onclick={() => (showFull = !showFull)}
												class="w-full border-t border-border bg-bg-secondary px-3 py-1.5 text-center text-[10px] font-medium text-accent hover:bg-bg-hover"
											>
												{showFull ? 'Collapse' : 'Show more...'}
											</button>
										{/if}
									{/if}
								{:else if block.type === 'Json'}
									<pre class="overflow-x-auto p-3 leading-relaxed"><code class="hljs">{@html highlightCode(JSON.stringify(block.data, null, 2), 'json')}</code></pre>
								{:else if block.type === 'Image'}
									<img src={block.url} alt={block.alt ?? ''} class="max-h-64 p-2" />
								{/if}
							{/each}
						{/if}
					</div>
				{/if}
			{/snippet}
		</ExpandableChip>
	{/if}
{/if}
