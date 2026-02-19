<script lang="ts">
import { onMount } from 'svelte';
import { renderMarkdown } from '../markdown';

const {
	onNavigate = (_path: string) => {},
	showUploadLink = true,
}: {
	onNavigate?: (path: string) => void;
	showUploadLink?: boolean;
} = $props();

let markdown = $state('');
let loading = $state(true);
let error = $state<string | null>(null);

onMount(() => {
	let cancelled = false;
	fetch('/docs', {
		headers: {
			Accept: 'text/markdown',
		},
	})
		.then(async (res) => {
			if (!res.ok) {
				throw new Error(`Failed to load docs (${res.status})`);
			}
			const body = await res.text();
			if (cancelled) return;
			markdown = body;
			loading = false;
		})
		.catch((e) => {
			if (cancelled) return;
			error = e instanceof Error ? e.message : 'Failed to load docs';
			loading = false;
		});

	return () => {
		cancelled = true;
	};
});
</script>

<svelte:head>
	<title>Docs - opensession.io</title>
</svelte:head>

<div class="mx-auto max-w-5xl">
	<div class="mb-4 flex flex-wrap items-center gap-2 border border-border bg-bg-secondary px-3 py-2 text-xs text-text-secondary">
		<span class="font-semibold text-text-primary">Quick Links</span>
		<button onclick={() => onNavigate('/')} class="text-accent hover:underline">Sessions</button>
		<button onclick={() => onNavigate('/dx')} class="text-accent hover:underline">DX Lab</button>
		{#if showUploadLink}
			<button onclick={() => onNavigate('/upload')} class="text-accent hover:underline">Upload</button>
		{/if}
	</div>

	{#if loading}
		<div class="border border-border bg-bg-secondary px-4 py-6 text-sm text-text-secondary">Loading docs...</div>
	{:else if error}
		<div class="border border-error/30 bg-error/10 px-4 py-6 text-sm text-error">{error}</div>
	{:else}
		<article class="prose prose-invert max-w-none text-sm leading-relaxed docs-markdown">
			{@html renderMarkdown(markdown)}
		</article>
	{/if}
</div>

<style>
	:global(.docs-markdown table) {
		width: 100%;
		border-collapse: collapse;
	}

	:global(.docs-markdown th),
	:global(.docs-markdown td) {
		border: 1px solid var(--border, #2d3748);
		padding: 0.4rem 0.5rem;
		text-align: left;
	}

	:global(.docs-markdown code) {
		font-size: 0.85em;
	}
</style>
