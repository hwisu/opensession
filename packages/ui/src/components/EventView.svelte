<script lang="ts">
import { CHIP_LABEL_MAX } from '../constants';
import {
	calcContentLength,
	findCodeStats,
	findFirstText,
	findJsonPayload,
	formatContentLength,
	getToolName,
	isToolError,
	truncate,
} from '../event-helpers';
import { highlightCode } from '../highlight';
import { isLongContent, renderMarkdown } from '../markdown';
import type { Event } from '../types';
import { isTeamTool } from '../types';
import CodeBlockView from './CodeBlockView.svelte';
import ContentBlockList from './ContentBlockList.svelte';
import DiffView from './DiffView.svelte';
import ExpandableChip from './ExpandableChip.svelte';
import {
	arrowLeftIcon,
	arrowRightIcon,
	chevronRightIcon,
	clipboardIcon,
	fileEditIcon,
	fileIcon,
	globeIcon,
	imageIcon,
	lightningIcon,
	listIcon,
	refreshIcon,
	searchIcon,
	sendIcon,
	taskEndIcon,
	taskStartIcon,
	terminalIcon,
	trashIcon,
	usersIcon,
} from './icons';
import TeamToolMeta from './TeamToolMeta.svelte';

const {
	event,
	pairedResult,
	resultOk = false,
}: { event: Event; pairedResult?: Event; resultOk?: boolean } = $props();

// --- Helpers ---
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

// --- Team tool detection ---
const isTeamToolCall = $derived(
	eventTypeName === 'ToolCall' &&
		'data' in event.event_type &&
		isTeamTool(getToolName(event.event_type)),
);
const teamToolName = $derived(isTeamToolCall ? getToolName(event.event_type) : '');
const teamPayload = $derived.by(() => {
	if (!isTeamToolCall) return null;
	return findJsonPayload(event.content.blocks);
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

const TEAM_TOOL_ICONS: Record<string, string> = {
	TaskCreate: clipboardIcon,
	TaskUpdate: refreshIcon,
	SendMessage: sendIcon,
	TeamCreate: usersIcon,
	TeamDelete: usersIcon,
	TaskList: listIcon,
	TaskGet: listIcon,
};

const chipIcon = $derived(
	isTeamToolCall
		? (TEAM_TOOL_ICONS[teamToolName] ?? lightningIcon)
		: eventTypeName === 'ToolCall'
			? lightningIcon
			: eventTypeName === 'ToolResult'
				? arrowLeftIcon
				: (ICON_MAP[eventTypeName] ?? '&#x2022;'),
);

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
			for (const block of event.content.blocks) {
				if (block.type === 'Text' && block.text.trim()) {
					return truncate(block.text.trim().split('\n')[0]);
				}
			}
			return '';
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
	isTeamToolCall
		? teamToolName
		: eventTypeName === 'ToolCall' || eventTypeName === 'ToolResult'
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
					<div class="border-l-2 border-l-green-400/30 pl-2 sm:pl-3 text-sm">
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
				<div class="min-w-0 flex-1 text-sm">
					<ContentBlockList blocks={event.content.blocks} bind:showFull showJson={true} />
				</div>
			</div>
		{:else}
			<div class="flex items-start gap-1.5 sm:gap-3">
				<span class="tui-badge tui-badge-agent mt-0.5 shrink-0">AGENT</span>
				{#if event.timestamp}
					<span class="mt-0.5 shrink-0 text-[10px] text-text-muted hidden sm:inline">{new Date(event.timestamp).toLocaleTimeString('en-US', { hour12: false })}</span>
				{/if}
				<div class="min-w-0 flex-1 text-sm">
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
				{#if isTeamToolCall}
					<TeamToolMeta toolName={teamToolName} payload={teamPayload} />
				{:else if isError}
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
				{#if pairedResult && pairedResult.content.blocks.length > 0}
					<div class="border-t border-border/50 mt-1">
						<div class="px-3 py-1 text-[10px] font-medium text-text-muted uppercase tracking-wider bg-bg-secondary/50">Result</div>
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
					</div>
				{/if}
			{/snippet}
		</ExpandableChip>
	{/if}
{/if}
