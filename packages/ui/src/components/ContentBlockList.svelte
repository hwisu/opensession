<script lang="ts">
import { highlightCode } from '../highlight';
import { extractStandaloneFencedCode, isLongContent, renderMarkdown } from '../markdown';
import type { ContentBlock } from '../types';
import CodeBlockView from './CodeBlockView.svelte';

let {
	blocks,
	showFull = $bindable(false),
	showJson = false,
}: {
	blocks: ContentBlock[];
	showFull?: boolean;
	showJson?: boolean;
} = $props();
</script>

{#each blocks as block}
	{#if block.type === 'Text'}
		{@const long = isLongContent(block.text, 30)}
		{@const fencedCode = extractStandaloneFencedCode(block.text)}
		{#if block.text.trim()}
			{#if fencedCode}
				<div class="my-2">
					<CodeBlockView code={fencedCode.code} language={fencedCode.language}
						startLine={1} bind:showFull />
				</div>
			{:else}
				<div class="md-content" class:ev-collapsed={long && !showFull}>
					{@html renderMarkdown(block.text)}
				</div>
			{/if}
			{#if long && !fencedCode}
				<button
					onclick={() => (showFull = !showFull)}
					class="mt-1 text-xs font-medium text-accent hover:underline"
				>
					{showFull ? 'Show less' : 'Show more...'}
				</button>
			{/if}
		{/if}
	{:else if block.type === 'Code'}
		<div class="my-2">
			<CodeBlockView code={block.code} language={block.language}
				startLine={block.start_line ?? 1} bind:showFull />
		</div>
	{:else if block.type === 'Image'}
		<img src={block.url} alt={block.alt ?? ''} class="mt-2 max-h-64" />
	{:else if block.type === 'Json' && showJson}
		<div class="my-2 overflow-hidden border border-border">
			<div class="code-header"><span>json</span></div>
			<pre class="overflow-x-auto bg-bg-primary p-3 text-xs leading-relaxed"><code class="hljs">{@html highlightCode(JSON.stringify(block.data, null, 2), 'json')}</code></pre>
		</div>
	{:else if block.type === 'File'}
		<div class="my-1 border border-border bg-bg-primary px-3 py-2 text-xs font-mono text-text-muted">
			{block.path}
		</div>
	{/if}
{/each}
