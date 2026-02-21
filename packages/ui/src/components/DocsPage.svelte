<script lang="ts">
import { onMount } from 'svelte';
import { renderMarkdown } from '../markdown';

const {
	onNavigate = (_path: string) => {},
}: {
	onNavigate?: (path: string) => void;
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
	fetch('/docs?format=markdown', {
		cache: 'no-store',
		headers: {
			Accept: 'text/markdown',
			'Cache-Control': 'no-cache',
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

<div class="docs-stage mx-auto max-w-[72rem] space-y-5" data-testid="docs-page">
	<section class="docs-hero border border-border p-4 sm:p-5">
		<div class="flex flex-wrap items-end justify-between gap-3">
			<div class="space-y-1">
				<p class="docs-kicker text-[11px] uppercase tracking-[0.12em] text-text-muted">Owner's Manual</p>
				<h1 class="docs-title text-3xl text-text-primary sm:text-4xl">{parsed.title}</h1>
				<p class="max-w-2xl text-xs text-text-secondary sm:text-sm">
					Goal-driven product manual with chapter navigation and runnable examples.
				</p>
			</div>

			<div class="flex flex-wrap items-center gap-2 text-xs">
				<button
					type="button"
					onclick={() => onNavigate('/sessions')}
					class="docs-nav-btn border border-border px-3 py-1 text-text-secondary transition-colors hover:border-accent hover:text-accent"
				>
					Sessions
				</button>
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
		<div class="docs-shell grid items-start gap-5 lg:grid-cols-[18.25rem_minmax(0,1fr)]" data-testid="docs-content">
			<aside
				data-testid="docs-toc"
				class="docs-toc hidden h-fit border border-border bg-bg-secondary/65 p-3 lg:block"
			>
				<p class="docs-toc-title mb-2 text-xs text-text-muted">Contents</p>
				<nav class="space-y-1.5">
					{#each parsed.chapters as chapter, idx}
						<a
							href={`#${chapter.slug}`}
							class="docs-toc-link block px-2 py-1 text-xs text-text-secondary transition-colors hover:text-text-primary"
						>
							<span aria-hidden="true" class="docs-toc-index mr-1.5 text-warning">{idx + 1}.</span>
							<span>{chapter.heading}</span>
						</a>
					{/each}
				</nav>
			</aside>

			<div class="space-y-4">
				{#if parsed.introMarkdown}
					<section class="docs-intro border border-border bg-bg-secondary/65 p-4 sm:p-5">
						<div class="prose prose-invert max-w-none text-sm leading-relaxed docs-markdown">
							{@html renderMarkdown(parsed.introMarkdown)}
						</div>
					</section>
				{/if}

				{#each parsed.chapters as chapter, idx}
					<section
						id={chapter.slug}
						data-testid="docs-chapter"
						class="docs-chapter border border-border bg-bg-secondary/65 p-4 sm:p-5"
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
						<h2 class="docs-chapter-heading mb-3 text-3xl text-text-primary sm:text-4xl">{chapter.heading}</h2>
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
	.docs-stage {
		position: relative;
		padding-bottom: 1.25rem;
		color: var(--color-text-primary);
	}

	.docs-stage::before {
		content: '';
		position: absolute;
		inset: 0;
		pointer-events: none;
		background:
			radial-gradient(80% 55% at 15% 2%, color-mix(in oklab, var(--color-accent) 14%, transparent), transparent),
			linear-gradient(180deg, transparent 0%, color-mix(in oklab, var(--color-bg-secondary) 30%, transparent) 100%);
		opacity: 0.36;
	}

	.docs-hero,
	.docs-shell {
		position: relative;
	}

	.docs-hero,
	.docs-toc,
	.docs-intro,
	.docs-chapter {
		border-color: var(--color-border);
		background: color-mix(in oklab, var(--color-bg-secondary) 65%, transparent);
		box-shadow: 0 18px 56px color-mix(in oklab, var(--color-bg-primary) 82%, transparent);
		color: var(--color-text-primary);
	}

	.docs-title,
	.docs-chapter-heading,
	.docs-toc-title {
		font-family: 'Iowan Old Style', 'Palatino Linotype', 'Book Antiqua', Palatino, serif;
		letter-spacing: -0.02em;
	}

	.docs-kicker {
		padding-left: 0.6rem;
		border-left: 1px solid var(--color-border-light);
		color: var(--color-text-muted);
	}

	.docs-nav-btn {
		border-color: var(--color-border);
		background: color-mix(in oklab, var(--color-bg-primary) 50%, transparent);
		color: var(--color-text-secondary);
	}

	.docs-toc {
		border-left: 2px solid color-mix(in oklab, var(--color-accent) 45%, var(--color-border));
		background: color-mix(in oklab, var(--color-bg-secondary) 75%, transparent);
	}

	.docs-toc-title {
		color: var(--color-text-muted);
		text-transform: uppercase;
		letter-spacing: 0.08em;
	}

	.docs-toc-index {
		color: var(--color-warning);
	}

	.docs-toc-link {
		border-left: 1px solid transparent;
		color: var(--color-text-secondary);
	}

	.docs-toc-link:hover {
		border-left-color: var(--color-accent);
		background: color-mix(in oklab, var(--color-bg-primary) 50%, transparent);
		color: var(--color-text-primary);
	}

	.docs-intro,
	.docs-chapter {
		position: relative;
	}

	.docs-intro::before,
	.docs-chapter::before {
		content: '';
		position: absolute;
		top: 0;
		left: 0;
		width: 100%;
		height: 2px;
		background: color-mix(in oklab, #ff4e4e 72%, var(--color-border-light));
	}

	.docs-title,
	.docs-chapter-heading {
		color: var(--color-text-primary);
	}

	.docs-stage :global(p),
	.docs-stage :global(li),
	.docs-stage :global(td),
	.docs-stage :global(th) {
		color: var(--color-text-primary);
	}

	.docs-stage :global(a) {
		color: var(--color-text-primary);
	}

	.docs-stage :global(a:hover) {
		color: var(--color-accent);
	}

	:global(.docs-markdown table) {
		width: 100%;
		border-collapse: collapse;
		background: color-mix(in oklab, var(--color-bg-primary) 55%, transparent);
	}

	:global(.docs-markdown th),
	:global(.docs-markdown td) {
		border: 1px solid var(--color-border);
		padding: 0.4rem 0.5rem;
		text-align: left;
	}

	:global(.docs-markdown code) {
		font-size: 0.85em;
		background: color-mix(in oklab, var(--color-bg-primary) 72%, transparent);
		padding: 0.1rem 0.22rem;
		color: var(--color-text-primary);
	}

	:global(.docs-markdown h3) {
		font-family: 'Iowan Old Style', 'Palatino Linotype', 'Book Antiqua', Palatino, serif;
		font-size: 1.2rem;
		font-weight: 600;
		color: var(--color-text-primary);
	}

	:global(.docs-markdown blockquote) {
		border-left: 2px solid var(--color-accent);
		padding-left: 0.7rem;
		color: var(--color-text-secondary);
	}

	@media (min-width: 1024px) {
		.docs-toc {
			position: sticky;
			top: 1.25rem;
			align-self: flex-start;
			max-height: calc(100vh - 5rem);
			overflow-y: auto;
		}
	}

	@media (max-width: 1024px) {
		.docs-chapter-heading {
			font-size: 2rem;
		}
	}
</style>
