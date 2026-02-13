<script lang="ts">
const { diff }: { diff: string } = $props();

interface DiffLine {
	type: 'add' | 'del' | 'ctx' | 'hunk' | 'header';
	text: string;
	oldNum: string;
	newNum: string;
}

const lines = $derived.by((): DiffLine[] => {
	const result: DiffLine[] = [];
	let oldLine = 0;
	let newLine = 0;

	for (const raw of diff.split('\n')) {
		if (raw.startsWith('@@')) {
			const match = raw.match(/@@ -(\d+)/);
			if (match) {
				oldLine = parseInt(match[1], 10);
				const newMatch = raw.match(/\+(\d+)/);
				newLine = newMatch ? parseInt(newMatch[1], 10) : oldLine;
			}
			result.push({ type: 'hunk', text: raw, oldNum: '', newNum: '' });
		} else if (
			raw.startsWith('---') ||
			raw.startsWith('+++') ||
			raw.startsWith('diff ') ||
			raw.startsWith('index ')
		) {
			result.push({ type: 'header', text: raw, oldNum: '', newNum: '' });
		} else if (raw.startsWith('+')) {
			result.push({ type: 'add', text: raw.slice(1), oldNum: '', newNum: String(newLine) });
			newLine++;
		} else if (raw.startsWith('-')) {
			result.push({ type: 'del', text: raw.slice(1), oldNum: String(oldLine), newNum: '' });
			oldLine++;
		} else {
			const text = raw.startsWith(' ') ? raw.slice(1) : raw;
			if (raw !== '' || result.length > 0) {
				result.push({
					type: 'ctx',
					text,
					oldNum: oldLine > 0 ? String(oldLine) : '',
					newNum: newLine > 0 ? String(newLine) : '',
				});
				if (oldLine > 0) oldLine++;
				if (newLine > 0) newLine++;
			}
		}
	}

	return result;
});
</script>

<div class="diff-view overflow-x-auto">
	{#each lines as line}
		{#if line.type === 'header'}
			<!-- skip file headers -->
		{:else if line.type === 'hunk'}
			<div class="diff-line diff-hunk">
				<div class="diff-gutter"></div>
				<div class="diff-gutter"></div>
				<div class="diff-text">{line.text}</div>
			</div>
		{:else}
			<div class="diff-line {line.type === 'add' ? 'diff-add' : line.type === 'del' ? 'diff-del' : 'diff-ctx'}">
				<div class="diff-gutter">{line.oldNum}</div>
				<div class="diff-gutter">{line.newNum}</div>
				<div class="diff-text">{line.text}</div>
			</div>
		{/if}
	{/each}
</div>
