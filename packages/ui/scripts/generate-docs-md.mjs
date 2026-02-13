#!/usr/bin/env node
// Converts DocsPage.svelte to Markdown for AI agent consumption.
// Usage: node generate-docs-md.mjs [output-path]
//   If output-path is omitted, writes to stdout.

import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { JSDOM } from 'jsdom';
import TurndownService from 'turndown';

const __dirname = dirname(fileURLToPath(import.meta.url));
const sveltePath = join(__dirname, '../src/components/DocsPage.svelte');

const raw = readFileSync(sveltePath, 'utf-8');

// 1. Strip <script> and <style> blocks
let html = raw.replace(/<script[\s\S]*?<\/script>/gi, '').replace(/<style[\s\S]*?<\/style>/gi, '');

// 2. Convert onNavigate buttons to <a> tags BEFORE stripping Svelte expressions
html = html.replace(
	/<button\s+onclick=\{?\(\)\s*=>\s*onNavigate\(['"]([^'"]+)['"]\)\}?[^>]*>([\s\S]*?)<\/button>/gi,
	'<a href="https://opensession.io$1">$2</a>',
);

// 3. Remove the ToC <nav> entirely (it relies on Svelte {#each} data)
html = html.replace(/<nav[\s\S]*?<\/nav>/gi, '');

// 4. Remove scrollTo buttons — keep inner text
html = html.replace(/<button\s+onclick[^>]*scrollTo[^>]*>([\s\S]*?)<\/button>/gi, '$1');

// Remove any remaining onclick buttons
html = html.replace(/<button\s+onclick[^>]*>([\s\S]*?)<\/button>/gi, '<span>$1</span>');

// 5. Remove Svelte template blocks
html = html.replace(/\{#each[^}]*\}/g, '');
html = html.replace(/\{\/each\}/g, '');
html = html.replace(/\{#if[^}]*\}/g, '');
html = html.replace(/\{:else[^}]*\}/g, '');
html = html.replace(/\{\/if\}/g, '');

// 6. Process backtick template literals — extract content, escape braces
html = html.replace(/\{`([^`]*)`\}/g, (_match, content) => {
	return content.replace(/\{/g, '&#123;').replace(/\}/g, '&#125;');
});

// 7. Remove remaining Svelte expressions {sec.flag}, etc.
html = html.replace(/\{[^}]*\}/g, '');

// 8. Remove flag markers (--init, --upload, etc.) that are section decorators
html = html.replace(/<div[^>]*class="[^"]*text-xs[^"]*text-accent[^"]*"[^>]*>--\w+<\/div>/gi, '');

// 9. Parse with jsdom + convert with turndown
const dom = new JSDOM(html);
const body = dom.window.document.body;

const turndown = new TurndownService({
	headingStyle: 'atx',
	codeBlockStyle: 'fenced',
	bulletListMarker: '-',
});

// Preserve <pre> blocks as fenced code
turndown.addRule('preCode', {
	filter: (node) => node.nodeName === 'PRE' && node.querySelector('code,span'),
	replacement: (_content, node) => {
		const text = node.textContent.trim();
		return `\n\n\`\`\`\n${text}\n\`\`\`\n\n`;
	},
});

// Preserve inline <code>
turndown.addRule('inlineCode', {
	filter: (node) => node.nodeName === 'CODE' && node.parentNode.nodeName !== 'PRE',
	replacement: (content) => `\`${content}\``,
});

// Convert <kbd> to inline code
turndown.addRule('kbd', {
	filter: 'kbd',
	replacement: (content) => `\`${content}\``,
});

let md = turndown.turndown(body.innerHTML);

// 10. Post-process cleanup
//     Remove excessive blank lines
md = md.replace(/\n{3,}/g, '\n\n');
//     Trim trailing whitespace on each line
md = md
	.split('\n')
	.map((line) => line.trimEnd())
	.join('\n');
//     Remove the "$ opensession docs" decorative header
md = md.replace(/^\$ opensession docs\n+/, '');
//     Remove the footer CTA section
md = md.replace(/\n---\n+Ready to get started\?[\s\S]*$/, '\n');
md = md.replace(/\nReady to get started\?[\s\S]*$/, '\n');
//     Ensure single trailing newline
md = `${md.trim()}\n`;

// 11. Output
const outPath = process.argv[2];
if (outPath) {
	writeFileSync(outPath, md, 'utf-8');
} else {
	process.stdout.write(md);
}
