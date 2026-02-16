import type { LanguageFn } from 'highlight.js';
import hljs from 'highlight.js/lib/core';
import bash from 'highlight.js/lib/languages/bash';
import cpp from 'highlight.js/lib/languages/cpp';
import css from 'highlight.js/lib/languages/css';
import diff from 'highlight.js/lib/languages/diff';
import go from 'highlight.js/lib/languages/go';
import java from 'highlight.js/lib/languages/java';
// Register commonly used languages
import javascript from 'highlight.js/lib/languages/javascript';
import json from 'highlight.js/lib/languages/json';
import kotlin from 'highlight.js/lib/languages/kotlin';
import markdown from 'highlight.js/lib/languages/markdown';
import python from 'highlight.js/lib/languages/python';
import ruby from 'highlight.js/lib/languages/ruby';
import rust from 'highlight.js/lib/languages/rust';
import sql from 'highlight.js/lib/languages/sql';
import swift from 'highlight.js/lib/languages/swift';
import typescript from 'highlight.js/lib/languages/typescript';
import xml from 'highlight.js/lib/languages/xml';
import yaml from 'highlight.js/lib/languages/yaml';
import { HIGHLIGHT_AUTO_MAX_CHARS } from './constants';

const HIGHLIGHT_CACHE_MAX_ENTRIES = 256;
const HIGHLIGHT_CACHE_MAX_CODE_CHARS = 150_000;
const highlightCache = new Map<string, string>();

const LANGUAGES: [string[], LanguageFn][] = [
	[['javascript', 'js'], javascript],
	[['typescript', 'ts'], typescript],
	[['python', 'py'], python],
	[['rust', 'rs'], rust],
	[['go'], go],
	[['bash', 'sh', 'shell'], bash],
	[['json'], json],
	[['css'], css],
	[['html', 'xml', 'svelte'], xml],
	[['sql'], sql],
	[['yaml', 'yml'], yaml],
	[['markdown', 'md'], markdown],
	[['diff'], diff],
	[['java'], java],
	[['kotlin', 'kt'], kotlin],
	[['swift'], swift],
	[['ruby', 'rb'], ruby],
	[['cpp', 'c'], cpp],
];

for (const [aliases, fn] of LANGUAGES) {
	for (const alias of aliases) hljs.registerLanguage(alias, fn);
}

/**
 * Highlight code with language detection.
 * Returns HTML string with syntax highlighting spans.
 */
export function highlightCode(code: string, language?: string | null): string {
	if (!code) return '';

	const lang = language?.toLowerCase();
	const cacheable = code.length <= HIGHLIGHT_CACHE_MAX_CODE_CHARS;
	const cacheKey = cacheable ? `${lang ?? ''}\u0000${code}` : null;
	if (cacheable && cacheKey) {
		const cached = highlightCache.get(cacheKey);
		if (cached !== undefined) {
			// Refresh insertion order to approximate LRU.
			highlightCache.delete(cacheKey);
			highlightCache.set(cacheKey, cached);
			return cached;
		}
	}

	let highlighted: string;

	if (lang && hljs.getLanguage(lang)) {
		try {
			highlighted = hljs.highlight(code, { language: lang }).value;
			if (cacheable && cacheKey) {
				putHighlightCache(cacheKey, highlighted);
			}
			return highlighted;
		} catch {
			// fallthrough to auto-detect
		}
	}

	// Auto-detect for short code snippets, plain text for long ones
	if (code.length < HIGHLIGHT_AUTO_MAX_CHARS) {
		try {
			highlighted = hljs.highlightAuto(code).value;
			if (cacheable && cacheKey) {
				putHighlightCache(cacheKey, highlighted);
			}
			return highlighted;
		} catch {
			highlighted = escapeHtml(code);
			if (cacheable && cacheKey) {
				putHighlightCache(cacheKey, highlighted);
			}
			return highlighted;
		}
	}

	highlighted = escapeHtml(code);
	if (cacheable && cacheKey) {
		putHighlightCache(cacheKey, highlighted);
	}
	return highlighted;
}

function escapeHtml(text: string): string {
	return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

function putHighlightCache(key: string, value: string): void {
	if (highlightCache.size >= HIGHLIGHT_CACHE_MAX_ENTRIES) {
		const firstKey = highlightCache.keys().next().value;
		if (firstKey !== undefined) highlightCache.delete(firstKey);
	}
	highlightCache.set(key, value);
}
