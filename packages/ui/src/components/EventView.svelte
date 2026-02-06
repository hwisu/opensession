<script lang="ts">
	import type { Event } from '../types';
	import { highlightCode } from '../highlight';
	import { renderMarkdown, isLongContent } from '../markdown';

	let { event }: { event: Event } = $props();

	// --- Helpers ---
	const COMPACT_ICONS: Record<string, { icon: string; color: string; label: string }> = {
		ToolCall: { icon: '>', color: 'bg-cyan-500/20 text-cyan-400', label: 'call' },
		ToolResult: { icon: '<', color: 'bg-cyan-500/20 text-cyan-400', label: 'result' },
		FileEdit: { icon: '~', color: 'bg-green-500/20 text-green-400', label: 'edit' },
		FileCreate: { icon: '+', color: 'bg-green-500/20 text-green-400', label: 'create' },
		FileDelete: { icon: 'x', color: 'bg-red-500/20 text-red-400', label: 'delete' },
		ShellCommand: { icon: '$', color: 'bg-yellow-500/20 text-yellow-400', label: 'shell' },
		ImageGenerate: { icon: 'I', color: 'bg-pink-500/20 text-pink-400', label: 'image' },
		WebSearch: { icon: '?', color: 'bg-indigo-500/20 text-indigo-400', label: 'search' },
		WebFetch: { icon: '@', color: 'bg-indigo-500/20 text-indigo-400', label: 'fetch' },
		FileRead: { icon: 'R', color: 'bg-blue-500/20 text-blue-400', label: 'read' },
		CodeSearch: { icon: '/', color: 'bg-cyan-500/20 text-cyan-400', label: 'search' },
		FileSearch: { icon: 'F', color: 'bg-teal-500/20 text-teal-400', label: 'find' },
		TaskStart: { icon: '▶', color: 'bg-gray-500/20 text-gray-400', label: 'task-start' },
		TaskEnd: { icon: '■', color: 'bg-gray-500/20 text-gray-400', label: 'task-end' }
	};

	function getCompactIcon(type: string) {
		return COMPACT_ICONS[type] ?? { icon: '*', color: 'bg-gray-500/20 text-gray-400', label: type };
	}

	function generateLineNumbers(code: string, startLine = 1): string {
		const lineCount = code.split('\n').length;
		const numbers: string[] = [];
		for (let i = startLine; i < startLine + lineCount; i++) {
			numbers.push(`<span>${i}</span>`);
		}
		return numbers.join('\n');
	}

	// --- Classify ---
	const evType = event.event_type.type;
	const isMessage = ['UserMessage', 'AgentMessage', 'SystemMessage'].includes(evType);
	const isThinking = evType === 'Thinking';
	const isSubAgent = evType === 'ToolCall' && event.event_type.data?.name === 'Task';

	// --- State ---
	let expanded = $state(isMessage ? true : false);
	let showFull = $state(false);

	// --- Derived ---
	let hasContent = $derived(event.content.blocks.length > 0);
	let eventModel = $derived(event.attributes?.model as string | undefined);

	// Does this event have any Code blocks? (parser already structured it)
	let hasCodeBlock = $derived(event.content.blocks.some((b) => b.type === 'Code'));

	let eventLabel = $derived.by(() => {
		const t = event.event_type;
		switch (t.type) {
			case 'ToolCall':
				// For Read/Glob/Grep: first Text block has the path/pattern
				if (event.content.blocks.length > 0 && event.content.blocks[0].type === 'Text') {
					return `${t.data.name}  ${event.content.blocks[0].text}`;
				}
				return t.data.name;
			case 'ToolResult':
				return t.data.name;
			case 'FileEdit':
				return t.data.path;
			case 'FileCreate':
				return t.data.path;
			case 'FileDelete':
				return t.data.path;
			case 'ShellCommand': {
				const cmd = t.data.command;
				return cmd.length > 80 ? cmd.slice(0, 77) + '...' : cmd;
			}
			case 'ImageGenerate':
				return t.data.prompt;
			case 'WebSearch':
				return t.data.query;
			case 'WebFetch':
				return t.data.url;
			case 'TaskStart':
				return t.data.title ?? '';
			case 'TaskEnd':
				return t.data.summary ?? '';
			case 'FileRead':
				return t.data.path;
			case 'CodeSearch':
				return t.data.query;
			case 'FileSearch':
				return t.data.pattern;
			default:
				return '';
		}
	});

	let isError = $derived(
		evType === 'ToolResult' && 'data' in event.event_type && event.event_type.data.is_error
	);

	let contentLength = $derived.by(() => {
		let len = 0;
		for (const block of event.content.blocks) {
			if (block.type === 'Text') len += block.text.length;
			else if (block.type === 'Code') len += block.code.length;
		}
		return len;
	});

	// Code block stats for ToolResult display
	let codeStats = $derived.by(() => {
		for (const block of event.content.blocks) {
			if (block.type === 'Code') {
				return {
					lines: block.code.split('\n').length,
					lang: block.language,
					startLine: block.start_line ?? 1
				};
			}
		}
		return null;
	});

	let subAgentDesc = $derived.by(() => {
		if (!isSubAgent) return null;
		for (const block of event.content.blocks) {
			if (block.type === 'Text') return block.text;
		}
		return null;
	});
</script>

<!-- ═══ MESSAGE (User / Agent / System) ═══ -->
{#if isMessage}
	{@const isUser = evType === 'UserMessage'}
	<div class="ev-message group my-4" data-event-type={evType}>
		{#if isUser}
			<!-- User message: prominent left-aligned bubble with label -->
			<div class="flex items-start gap-3">
				<div class="mt-1 flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-blue-500/30 text-xs font-bold text-blue-300">
					U
				</div>
				<div class="min-w-0 flex-1">
					<p class="mb-1 text-xs font-medium text-blue-400">User</p>
					<div class="rounded-xl bg-blue-500/10 border border-blue-500/20 px-4 py-3 text-sm">
						{#each event.content.blocks as block}
							{#if block.type === 'Text'}
								{@const long = isLongContent(block.text, 30)}
								{#if block.text.trim()}
									<div class="md-content" class:ev-collapsed={long && !showFull}>
										{@html renderMarkdown(block.text)}
									</div>
									{#if long}
										<button
											onclick={() => (showFull = !showFull)}
											class="mt-1 text-xs font-medium text-accent hover:underline"
										>
											{showFull ? 'Show less' : 'Show more...'}
										</button>
									{/if}
								{/if}
							{:else if block.type === 'Code'}
								<div class="my-2 overflow-hidden rounded-lg border border-border">
									{#if block.language}
										<div class="code-header"><span>{block.language}</span></div>
									{/if}
									<div class="code-with-lines">
										<pre class="line-nums">{@html generateLineNumbers(block.code, block.start_line ?? 1)}</pre>
										<pre class="code-body"><code class="hljs">{@html highlightCode(block.code, block.language)}</code></pre>
									</div>
								</div>
							{:else if block.type === 'Image'}
								<img src={block.url} alt={block.alt ?? ''} class="mt-2 max-h-64 rounded-lg" />
							{:else if block.type === 'File'}
								<div class="my-1 rounded border border-border bg-bg-primary px-3 py-2 text-xs font-mono text-text-muted">
									{block.path}
								</div>
							{/if}
						{/each}
					</div>
				</div>
			</div>
		{:else}
			<!-- AI message: clean left-aligned, no heavy badge -->
			<div class="ml-11">
				{#if eventModel}
					<p class="mb-1 text-[10px] font-medium uppercase tracking-wider text-text-muted">
						{eventModel}
					</p>
				{/if}
				<div class="rounded-xl bg-bg-tertiary border border-border px-4 py-3 text-sm">
					{#each event.content.blocks as block}
						{#if block.type === 'Text'}
							{@const long = isLongContent(block.text, 30)}
							{#if block.text.trim()}
								<div class="md-content" class:ev-collapsed={long && !showFull}>
									{@html renderMarkdown(block.text)}
								</div>
								{#if long}
									<button
										onclick={() => (showFull = !showFull)}
										class="mt-1 text-xs font-medium text-accent hover:underline"
									>
										{showFull ? 'Show less' : 'Show more...'}
									</button>
								{/if}
							{/if}
						{:else if block.type === 'Code'}
							<div class="my-2 overflow-hidden rounded-lg border border-border">
								{#if block.language}
									<div class="code-header"><span>{block.language}</span></div>
								{/if}
								<div class="code-with-lines">
									<pre class="line-nums">{@html generateLineNumbers(block.code, block.start_line ?? 1)}</pre>
									<pre class="code-body"><code class="hljs">{@html highlightCode(block.code, block.language)}</code></pre>
								</div>
							</div>
						{:else if block.type === 'Image'}
							<img src={block.url} alt={block.alt ?? ''} class="mt-2 max-h-64 rounded-lg" />
						{:else if block.type === 'Json'}
							<div class="my-2 overflow-hidden rounded-lg border border-border">
								<div class="code-header"><span>json</span></div>
								<pre class="overflow-x-auto bg-bg-primary p-3 text-xs leading-relaxed"><code class="hljs">{@html highlightCode(JSON.stringify(block.data, null, 2), 'json')}</code></pre>
							</div>
						{:else if block.type === 'File'}
							<div class="my-1 rounded border border-border bg-bg-primary px-3 py-2 text-xs font-mono text-text-muted">
								{block.path}
							</div>
						{/if}
					{/each}
				</div>
			</div>
		{/if}
	</div>

<!-- ═══ THINKING ═══ -->
{:else if isThinking}
	<div class="ev-thinking my-0.5" data-event-type="Thinking">
		<button
			onclick={() => (expanded = !expanded)}
			class="group flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs transition-colors hover:bg-bg-hover"
		>
			<span class="flex h-5 w-5 shrink-0 items-center justify-center rounded bg-purple-500/20 text-[10px] text-purple-400">T</span>
			<span class="flex-1 truncate font-mono text-text-muted group-hover:text-text-secondary">Thinking</span>
			{#if event.duration_ms}
				<span class="shrink-0 font-mono text-[10px] text-text-muted">{event.duration_ms}ms</span>
			{/if}
			<span class="shrink-0 text-text-muted">{expanded ? '▾' : '▸'}</span>
		</button>

		{#if expanded && hasContent}
			<div class="ml-7 mt-1 rounded-lg border border-purple-500/20 bg-purple-500/5 p-3 text-xs">
				{#each event.content.blocks as block}
					{#if block.type === 'Text'}
						{@const long = isLongContent(block.text)}
						{#if block.text.trim()}
							<div
								class="whitespace-pre-wrap break-words leading-relaxed text-purple-300/80"
								class:ev-collapsed={long && !showFull}
							>
								{block.text}
							</div>
							{#if long}
								<button
									onclick={() => (showFull = !showFull)}
									class="mt-1 text-[10px] font-medium text-purple-400 hover:underline"
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
	<div class="ev-subagent my-2" data-event-type="ToolCall">
		<button
			onclick={() => (expanded = !expanded)}
			class="group flex w-full items-center gap-2 border border-accent/30 bg-accent/10 px-3 py-2 text-left transition-colors hover:bg-accent/15
				{expanded ? 'rounded-t-lg' : 'rounded-lg'}"
		>
			<span class="flex h-6 w-6 shrink-0 items-center justify-center rounded bg-accent/30 text-xs font-bold text-accent">&#x2192;</span>
			<span class="flex-1 text-sm font-medium text-accent">Sub-agent</span>
			{#if event.duration_ms}
				<span class="font-mono text-[10px] text-text-muted">{event.duration_ms}ms</span>
			{/if}
			<span class="text-text-muted">{expanded ? '▾' : '▸'}</span>
		</button>

		{#if expanded}
			<div class="rounded-b-lg border border-t-0 border-accent/20 bg-bg-secondary p-3">
				{#if subAgentDesc}
					<div class="md-content text-sm text-text-secondary">
						{@html renderMarkdown(subAgentDesc)}
					</div>
				{/if}
				{#each event.content.blocks as block}
					{#if block.type === 'Code'}
						<div class="mt-2 overflow-hidden rounded-lg border border-border">
							{#if block.language}
								<div class="code-header"><span>{block.language}</span></div>
							{/if}
							<pre class="overflow-x-auto bg-bg-primary p-3 text-xs leading-relaxed"><code class="hljs">{@html highlightCode(block.code, block.language)}</code></pre>
						</div>
					{/if}
				{/each}
			</div>
		{/if}
	</div>

<!-- ═══ TOOL RESULT with Code block (parser already structured it) ═══ -->
{:else if evType === 'ToolResult' && hasCodeBlock}
	<div class="ev-compact my-0.5" data-event-type="ToolResult">
		<button
			onclick={() => (expanded = !expanded)}
			class="group flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs transition-colors hover:bg-bg-hover"
		>
			<span class="flex h-5 w-5 shrink-0 items-center justify-center rounded bg-cyan-500/20 text-[10px] text-cyan-400">&lt;</span>
			<span class="shrink-0 font-medium text-text-muted">result</span>
			<span class="min-w-0 flex-1 truncate font-mono text-text-secondary group-hover:text-text-primary">{eventLabel}</span>
			{#if isError}
				<span class="shrink-0 rounded bg-red-500/20 px-1.5 py-0.5 text-[10px] text-red-400">error</span>
			{/if}
			{#if codeStats}
				<span class="shrink-0 font-mono text-[10px] text-text-muted">{codeStats.lines} lines</span>
			{/if}
			<span class="shrink-0 text-text-muted">{expanded ? '▾' : '▸'}</span>
			{#if event.duration_ms}
				<span class="shrink-0 font-mono text-[10px] text-text-muted">{event.duration_ms}ms</span>
			{/if}
		</button>

		{#if expanded}
			<div class="ml-7 mt-1 overflow-hidden rounded-lg border border-border bg-bg-primary text-xs">
				{#each event.content.blocks as block}
					{#if block.type === 'Code'}
						{@const long = block.code.split('\n').length > 30}
						{#if block.language}
							<div class="code-header"><span>{block.language}</span></div>
						{/if}
						<div class:ev-collapsed-code={long && !showFull}>
							<div class="code-with-lines">
								<pre class="line-nums">{@html generateLineNumbers(block.code, block.start_line ?? 1)}</pre>
								<pre class="code-body"><code class="hljs">{@html highlightCode(block.code, block.language)}</code></pre>
							</div>
						</div>
						{#if long}
							<button
								onclick={() => (showFull = !showFull)}
								class="w-full border-t border-border bg-bg-secondary px-3 py-1.5 text-center text-[10px] font-medium text-accent hover:bg-bg-hover"
							>
								{showFull ? 'Collapse' : `Show all (${block.code.split('\n').length} lines)`}
							</button>
						{/if}
					{:else if block.type === 'Text'}
						{#if block.text.trim()}
							<div class="md-content p-3 text-text-secondary">
								{@html renderMarkdown(block.text)}
							</div>
						{/if}
					{/if}
				{/each}
			</div>
		{/if}
	</div>

<!-- ═══ TOOL / FILE / SHELL / OTHER ═══ -->
{:else}
	{@const iconInfo = getCompactIcon(evType)}

	<div class="ev-compact my-0.5" data-event-type={evType}>
		<button
			onclick={() => (expanded = !expanded)}
			class="group flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs transition-colors hover:bg-bg-hover"
		>
			<span class="flex h-5 w-5 shrink-0 items-center justify-center rounded text-[10px] {iconInfo.color}">
				{iconInfo.icon}
			</span>
			<span class="shrink-0 font-medium text-text-muted">{iconInfo.label}</span>
			<span class="min-w-0 flex-1 truncate font-mono text-text-secondary group-hover:text-text-primary">{eventLabel}</span>
			{#if isError}
				<span class="shrink-0 rounded bg-red-500/20 px-1.5 py-0.5 text-[10px] text-red-400">error</span>
			{/if}
			{#if hasContent && contentLength > 0}
				<span class="shrink-0 font-mono text-[10px] text-text-muted"
					>{contentLength > 1000 ? `${Math.round(contentLength / 1000)}k` : contentLength} chars</span
				>
			{/if}
			{#if hasContent}
				<span class="shrink-0 text-text-muted">{expanded ? '▾' : '▸'}</span>
			{/if}
		</button>

		{#if expanded && hasContent}
			<div class="ml-7 mt-1 overflow-hidden rounded-lg border border-border bg-bg-primary text-xs">
				{#each event.content.blocks as block}
					{#if block.type === 'Code'}
						{@const long = isLongContent(block.code, 40)}
						{#if block.language}
							<div class="code-header"><span>{block.language}</span></div>
						{/if}
						<div class:ev-collapsed-code={long && !showFull}>
							<div class="code-with-lines">
								<pre class="line-nums">{@html generateLineNumbers(block.code, block.start_line ?? 1)}</pre>
								<pre class="code-body"><code class="hljs">{@html highlightCode(block.code, block.language)}</code></pre>
							</div>
						</div>
						{#if long}
							<button
								onclick={() => (showFull = !showFull)}
								class="w-full border-t border-border bg-bg-secondary px-3 py-1.5 text-center text-[10px] font-medium text-accent hover:bg-bg-hover"
							>
								{showFull ? 'Collapse' : `Show all (${block.code.split('\n').length} lines)`}
							</button>
						{/if}
					{:else if block.type === 'Text'}
						{@const long = isLongContent(block.text)}
						{#if block.text.trim()}
							<div
								class="md-content p-3 text-text-secondary"
								class:ev-collapsed={long && !showFull}
							>
								{@html renderMarkdown(block.text)}
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
						<img src={block.url} alt={block.alt ?? ''} class="max-h-64 rounded p-2" />
					{:else if block.type === 'File'}
						<div class="p-3 font-mono text-text-muted">{block.path}</div>
					{/if}
				{/each}
			</div>
		{/if}
	</div>
{/if}

<style>
	.ev-collapsed {
		max-height: 24rem;
		overflow: hidden;
		mask-image: linear-gradient(to bottom, black 70%, transparent 100%);
		-webkit-mask-image: linear-gradient(to bottom, black 70%, transparent 100%);
	}
	.ev-collapsed-code {
		max-height: 20rem;
		overflow: hidden;
		mask-image: linear-gradient(to bottom, black 80%, transparent 100%);
		-webkit-mask-image: linear-gradient(to bottom, black 80%, transparent 100%);
	}
</style>
