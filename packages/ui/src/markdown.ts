import { Marked, type RendererThis, type Token, type Tokens } from 'marked';
import { LONG_CONTENT_CHAR_THRESHOLD, LONG_TEXT_LINE_THRESHOLD } from './constants';
import { highlightCode } from './highlight';

const MARKDOWN_CACHE_MAX_ENTRIES = 256;
const MARKDOWN_CACHE_MAX_CHARS = 150_000;
const markdownCache = new Map<string, string>();

function parseInlineText(ctx: RendererThis, token: { text: string; tokens?: Token[] }): string {
	if (token.tokens && ctx.parser?.parseInline) {
		return ctx.parser.parseInline(token.tokens);
	}
	return token.text;
}

const marked = new Marked({
	gfm: true,
	breaks: true,
	renderer: {
		code({ text, lang }: Tokens.Code) {
			const highlighted = highlightCode(text, lang);
			const langLabel = lang
				? `<div class="md-code-header"><span>${escapeHtml(lang)}</span></div>`
				: '';
			return `<div class="md-code-block">${langLabel}<pre><code class="hljs">${highlighted}</code></pre></div>`;
		},
		codespan({ text }: Tokens.Codespan) {
			return `<code class="md-inline-code">${text}</code>`;
		},
		heading(this: RendererThis, token: Tokens.Heading) {
			const text = parseInlineText(this, token);
			return `<h${token.depth} class="md-heading md-h${token.depth}">${text}</h${token.depth}>`;
		},
		paragraph(this: RendererThis, token: Tokens.Paragraph) {
			const text = parseInlineText(this, token);
			return `<p class="md-p">${text}</p>`;
		},
		list(this: RendererThis, token: Tokens.List) {
			const tag = token.ordered ? 'ol' : 'ul';
			const items = token.items
				.map((item: Tokens.ListItem) => `<li>${parseInlineText(this, item)}</li>`)
				.join('');
			return `<${tag} class="md-list">${items}</${tag}>`;
		},
		blockquote(this: RendererThis, token: Tokens.Blockquote) {
			const text =
				token.tokens && this.parser?.parse ? this.parser.parse(token.tokens) : token.text;
			return `<blockquote class="md-blockquote">${text}</blockquote>`;
		},
		link({ href, text }: Tokens.Link) {
			return `<a href="${href}" class="md-link" target="_blank" rel="noopener">${text}</a>`;
		},
		table(this: RendererThis, token: Tokens.Table) {
			const headerCells = token.header
				.map((cell: Tokens.TableCell) => `<th>${parseInlineText(this, cell)}</th>`)
				.join('');
			const rows = token.rows
				.map(
					(row: Tokens.TableCell[]) =>
						`<tr>${row.map((cell: Tokens.TableCell) => `<td>${parseInlineText(this, cell)}</td>`).join('')}</tr>`,
				)
				.join('');
			return `<div class="md-table-wrap"><table class="md-table"><thead><tr>${headerCells}</tr></thead><tbody>${rows}</tbody></table></div>`;
		},
		hr() {
			return '<hr class="md-hr" />';
		},
		image({ href, text }: Tokens.Image) {
			return `<img src="${href}" alt="${text ?? ''}" class="md-img" />`;
		},
	},
});

/**
 * Render markdown string to HTML.
 * Uses marked with GFM + highlight.js for code blocks.
 */
export function renderMarkdown(text: string): string {
	if (!text) return '';
	const cacheable = text.length <= MARKDOWN_CACHE_MAX_CHARS;
	if (cacheable) {
		const cached = markdownCache.get(text);
		if (cached !== undefined) {
			// Refresh insertion order to keep a tiny LRU behavior.
			markdownCache.delete(text);
			markdownCache.set(text, cached);
			return cached;
		}
	}

	let rendered: string;
	try {
		rendered = marked.parse(text) as string;
	} catch {
		rendered = escapeHtml(text);
	}

	if (cacheable) {
		if (markdownCache.size >= MARKDOWN_CACHE_MAX_ENTRIES) {
			const firstKey = markdownCache.keys().next().value;
			if (firstKey !== undefined) markdownCache.delete(firstKey);
		}
		markdownCache.set(text, rendered);
	}
	return rendered;
}

/**
 * Parse a text blob that contains only a single fenced code block.
 * Returns null when the text has additional prose/markdown around the code.
 */
export function extractStandaloneFencedCode(
	text: string,
): { code: string; language?: string } | null {
	if (!text) return null;
	const normalized = text.replace(/\r\n?/g, '\n').trim();
	if (!normalized) return null;

	// ```lang\n...\n``` or ~~~lang\n...\n~~~
	const match = normalized.match(
		/^(?<fence>`{3,}|~{3,})(?<lang>[^\n`]*)\n(?<code>[\s\S]*?)\n\k<fence>$/,
	);
	if (!match?.groups) return null;

	const code = match.groups.code ?? '';
	const language = (match.groups.lang ?? '').trim();
	return {
		code,
		language: language.length > 0 ? language : undefined,
	};
}

function escapeHtml(text: string): string {
	return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

/** Count approximate lines in text */
export function lineCount(text: string): number {
	return text.split('\n').length;
}

/** Check if content is "long" and should be collapsed */
export function isLongContent(text: string, threshold: number = LONG_TEXT_LINE_THRESHOLD): boolean {
	return lineCount(text) > threshold || text.length > LONG_CONTENT_CHAR_THRESHOLD;
}
