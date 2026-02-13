import { Marked, type Tokens } from 'marked';
import { LONG_CONTENT_CHAR_THRESHOLD, LONG_TEXT_LINE_THRESHOLD } from './constants';
import { highlightCode } from './highlight';

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
		heading({ text, depth }: Tokens.Heading) {
			return `<h${depth} class="md-heading md-h${depth}">${text}</h${depth}>`;
		},
		paragraph({ text }: Tokens.Paragraph) {
			return `<p class="md-p">${text}</p>`;
		},
		list(token: Tokens.List) {
			const tag = token.ordered ? 'ol' : 'ul';
			const items = token.items.map((item: Tokens.ListItem) => `<li>${item.text}</li>`).join('');
			return `<${tag} class="md-list">${items}</${tag}>`;
		},
		blockquote({ text }: Tokens.Blockquote) {
			return `<blockquote class="md-blockquote">${text}</blockquote>`;
		},
		link({ href, text }: Tokens.Link) {
			return `<a href="${href}" class="md-link" target="_blank" rel="noopener">${text}</a>`;
		},
		table(token: Tokens.Table) {
			const headerCells = token.header
				.map((cell: Tokens.TableCell) => `<th>${cell.text}</th>`)
				.join('');
			const rows = token.rows
				.map(
					(row: Tokens.TableCell[]) =>
						`<tr>${row.map((cell: Tokens.TableCell) => `<td>${cell.text}</td>`).join('')}</tr>`,
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
	try {
		return marked.parse(text) as string;
	} catch {
		return escapeHtml(text);
	}
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
