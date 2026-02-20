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

type DocsChapter = {
	heading: string;
	slug: string;
	markdown: string;
};

type ParsedDocs = {
	title: string;
	introMarkdown: string;
	chapters: DocsChapter[];
};

let loading = $state(true);
let error = $state<string | null>(null);
let parsed = $state<ParsedDocs>({
	title: 'Documentation',
	introMarkdown: '',
	chapters: [],
});

function slugify(value: string): string {
	const trimmed = value.trim().toLowerCase();
	const slug = trimmed
		.replace(/[^a-z0-9\s-]/g, '')
		.replace(/\s+/g, '-')
		.replace(/-+/g, '-')
		.replace(/^-|-$/g, '');
	return slug || 'chapter';
}

function parseDocsMarkdown(markdown: string): ParsedDocs {
	const lines = markdown.split(/\r?\n/);
	let title = 'Documentation';
	let startIndex = 0;
	const titleMatch = lines[0]?.match(/^#\s+(.+?)\s*$/);
	if (titleMatch) {
		title = titleMatch[1].trim();
		startIndex = 1;
	}

	const introLines: string[] = [];
	const chapters: Array<{ heading: string; lines: string[] }> = [];
	let currentHeading: string | null = null;
	let currentLines: string[] = [];

	const flushChapter = () => {
		if (!currentHeading) return;
		chapters.push({
			heading: currentHeading,
			lines: currentLines,
		});
	};

	for (const line of lines.slice(startIndex)) {
		const chapterMatch = line.match(/^##\s+(.+?)\s*$/);
		if (chapterMatch) {
			flushChapter();
			currentHeading = chapterMatch[1].trim();
			currentLines = [];
			continue;
		}

		if (currentHeading) {
			currentLines.push(line);
		} else {
			introLines.push(line);
		}
	}
	flushChapter();

	const slugCounts = new Map<string, number>();
	const normalizedChapters: DocsChapter[] = chapters.map((chapter) => {
		const baseSlug = slugify(chapter.heading);
		const currentCount = slugCounts.get(baseSlug) ?? 0;
		slugCounts.set(baseSlug, currentCount + 1);
		const slug = currentCount === 0 ? baseSlug : `${baseSlug}-${currentCount + 1}`;

		return {
			heading: chapter.heading,
			slug,
			markdown: chapter.lines.join('\n').trim(),
		};
	});

	return {
		title,
		introMarkdown: introLines.join('\n').trim(),
		chapters: normalizedChapters,
	};
}

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
				parsed = parseDocsMarkdown(body);
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

<div class="mx-auto max-w-6xl space-y-4" data-testid="docs-page">
	<section class="border border-border bg-bg-secondary p-4 sm:p-5">
		<div class="flex flex-wrap items-center justify-between gap-3">
			<div class="space-y-1">
				<p class="text-[11px] uppercase tracking-[0.12em] text-text-muted">docs</p>
				<h1 class="text-xl font-semibold text-text-primary sm:text-2xl">{parsed.title}</h1>
				<p class="text-xs text-text-secondary">
					Capability-aware product guide with chapter navigation and runnable examples.
				</p>
			</div>

			<div class="flex flex-wrap items-center gap-2 text-xs">
				<button
					type="button"
					onclick={() => onNavigate('/')}
					class="border border-border px-3 py-1 text-text-secondary transition-colors hover:border-accent hover:text-accent"
				>
					Sessions
				</button>
				{#if showUploadLink}
					<button
						type="button"
						onclick={() => onNavigate('/upload')}
						class="border border-border px-3 py-1 text-text-secondary transition-colors hover:border-accent hover:text-accent"
					>
						Upload
					</button>
				{/if}
			</div>
		</div>
	</section>

	{#if loading}
		<div class="border border-border bg-bg-secondary px-4 py-6 text-sm text-text-secondary">
			Loading docs...
		</div>
	{:else if error}
		<div class="border border-error/30 bg-error/10 px-4 py-6 text-sm text-error">{error}</div>
	{:else if parsed.chapters.length === 0}
		<div class="border border-warning/30 bg-warning/10 px-4 py-6 text-sm text-warning">
			No documentation chapters were found.
		</div>
	{:else}
		<div class="grid gap-4 lg:grid-cols-[17rem_minmax(0,1fr)]" data-testid="docs-content">
			<aside
				data-testid="docs-toc"
				class="hidden h-fit border border-border bg-bg-secondary p-3 lg:sticky lg:top-4 lg:block"
			>
				<p class="mb-2 text-[11px] uppercase tracking-[0.1em] text-text-muted">Chapters</p>
				<nav class="space-y-1.5">
					{#each parsed.chapters as chapter}
						<a
							href={`#${chapter.slug}`}
							class="block border border-transparent px-2 py-1 text-xs text-text-secondary transition-colors hover:border-border hover:bg-bg-primary hover:text-text-primary"
						>
							{chapter.heading}
						</a>
					{/each}
				</nav>
			</aside>

			<div class="space-y-4">
				{#if parsed.introMarkdown}
					<section class="border border-border bg-bg-secondary p-4">
						<div class="prose prose-invert max-w-none text-sm leading-relaxed docs-markdown">
							{@html renderMarkdown(parsed.introMarkdown)}
						</div>
					</section>
				{/if}

				{#each parsed.chapters as chapter, idx}
					<section
						id={chapter.slug}
						data-testid="docs-chapter"
						class="border border-border bg-bg-secondary p-4 sm:p-5"
					>
						<div class="mb-2 flex items-center justify-between gap-2">
							<p class="text-[11px] uppercase tracking-[0.1em] text-text-muted">
								Chapter {idx + 1}
							</p>
							<a
								href={`#${chapter.slug}`}
								class="text-[11px] text-text-muted transition-colors hover:text-accent"
							>
								#{chapter.slug}
							</a>
						</div>
						<h2 class="mb-3 text-xl font-semibold text-text-primary">{chapter.heading}</h2>
						<div class="prose prose-invert max-w-none text-sm leading-relaxed docs-markdown">
							{@html renderMarkdown(chapter.markdown)}
						</div>
					</section>
				{/each}
			</div>
		</div>
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
