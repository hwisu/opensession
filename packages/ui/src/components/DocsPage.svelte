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
		color: #131313;
	}

	.docs-stage::before {
		content: '';
		position: absolute;
		inset: 0;
		pointer-events: none;
		background:
			radial-gradient(84% 62% at 10% 0%, rgba(234, 232, 224, 0.34), transparent),
			linear-gradient(180deg, rgba(246, 244, 237, 0.22) 0%, rgba(243, 240, 229, 0.08) 100%);
		opacity: 0.9;
	}

	.docs-hero,
	.docs-shell {
		position: relative;
	}

	.docs-hero,
	.docs-toc,
	.docs-intro,
	.docs-chapter {
		border-color: color-mix(in oklab, #a39a84 55%, var(--color-border));
		background: color-mix(in oklab, #eeebe0 90%, transparent);
		box-shadow: 0 8px 26px rgba(17, 20, 27, 0.12);
		color: #161616;
	}

	.docs-title,
	.docs-chapter-heading,
	.docs-toc-title {
		font-family: 'Iowan Old Style', 'Palatino Linotype', 'Book Antiqua', Palatino, serif;
		letter-spacing: -0.02em;
	}

	.docs-kicker {
		padding-left: 0.6rem;
		border-left: 1px solid color-mix(in oklab, #928a79 62%, var(--color-border-light));
		color: #66604f;
	}

	.docs-nav-btn {
		border-color: color-mix(in oklab, #9b8f73 60%, var(--color-border));
		background: color-mix(in oklab, #ebe6d8 94%, transparent);
		color: #28231b;
	}

	.docs-toc {
		border-left: 2px solid color-mix(in oklab, #ad8769 68%, var(--color-border));
		background: color-mix(in oklab, #f2efe6 95%, transparent);
		box-shadow: 0 8px 24px rgba(19, 19, 19, 0.14);
	}

	.docs-toc-title {
		color: #60594b;
		text-transform: uppercase;
		letter-spacing: 0.08em;
	}

	.docs-toc-index {
		color: #cf4e42;
	}

	.docs-toc-link {
		border-left: 1px solid transparent;
		color: #322d23;
	}

	.docs-toc-link:hover {
		border-left-color: #cf4e42;
		background: color-mix(in oklab, #e4ded0 86%, transparent);
		color: #151515;
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
		background: color-mix(in oklab, #db4e43 78%, var(--color-border-light));
	}

	.docs-title,
	.docs-chapter-heading {
		color: #131210;
	}

	.docs-stage :global(p),
	.docs-stage :global(li),
	.docs-stage :global(td),
	.docs-stage :global(th) {
		color: #1f1d19;
	}

	.docs-stage :global(a) {
		color: #302a21;
	}

	.docs-stage :global(a:hover) {
		color: #10100f;
	}

	:global(.docs-markdown table) {
		width: 100%;
		border-collapse: collapse;
		background: color-mix(in oklab, #e6e1d3 72%, transparent);
	}

	:global(.docs-markdown th),
	:global(.docs-markdown td) {
		border: 1px solid color-mix(in oklab, #9a8f79 58%, var(--color-border));
		padding: 0.4rem 0.5rem;
		text-align: left;
	}

	:global(.docs-markdown code) {
		font-size: 0.85em;
		background: color-mix(in oklab, #ddd6c5 72%, transparent);
		padding: 0.1rem 0.22rem;
		color: #1f1f1f;
	}

	:global(.docs-markdown h3) {
		font-family: 'Iowan Old Style', 'Palatino Linotype', 'Book Antiqua', Palatino, serif;
		font-size: 1.2rem;
		font-weight: 600;
		color: #121110;
	}

	:global(.docs-markdown blockquote) {
		border-left: 2px solid #d4584c;
		padding-left: 0.7rem;
		color: #423b30;
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
