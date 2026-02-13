<script lang="ts">
import { LONG_CODE_LINE_THRESHOLD } from '../constants';
import { highlightCode } from '../highlight';
import { isLongContent } from '../markdown';

let {
	code,
	language,
	startLine = 1,
	showFull = $bindable(false),
}: {
	code: string;
	language?: string | null;
	startLine?: number;
	showFull?: boolean;
} = $props();

function generateLineNumbers(src: string, start: number): string {
	const count = src.split('\n').length;
	const numbers: string[] = [];
	for (let i = start; i < start + count; i++) {
		numbers.push(`<span>${i}</span>`);
	}
	return numbers.join('\n');
}

const long = $derived(isLongContent(code, LONG_CODE_LINE_THRESHOLD));
const lineCount = $derived(code.split('\n').length);
</script>

<div class="overflow-hidden border border-border">
	{#if language}
		<div class="code-header"><span>{language}</span></div>
	{/if}
	<div class:ev-collapsed-code={long && !showFull}>
		<div class="code-with-lines">
			<pre class="line-nums">{@html generateLineNumbers(code, startLine)}</pre>
			<pre class="code-body"><code class="hljs">{@html highlightCode(code, language)}</code></pre>
		</div>
	</div>
	{#if long}
		<button
			onclick={() => (showFull = !showFull)}
			class="w-full border-t border-border bg-bg-secondary px-3 py-1.5 text-center text-[10px] font-medium text-accent hover:bg-bg-hover"
		>
			{showFull ? 'Collapse' : `Show all (${lineCount} lines)`}
		</button>
	{/if}
</div>
