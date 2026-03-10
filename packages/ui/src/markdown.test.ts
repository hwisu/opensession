import assert from 'node:assert/strict';
import test from 'node:test';
import { renderMarkdown } from './markdown.ts';

test('renderMarkdown strips active script payloads', () => {
	const html = renderMarkdown('hello <script>alert(1)</script> world');
	assert.equal(html.includes('<script>'), false);
	assert.equal(html.includes('<script'), false);
	assert.equal(html.includes('hello'), true);
	assert.equal(html.includes('world'), true);
});

test('renderMarkdown blocks javascript and data URLs', () => {
	const jsLink = renderMarkdown('[xss](javascript:alert(1))');
	assert.equal(jsLink.includes('href='), false);
	assert.equal(jsLink.includes('javascript:'), false);

	const dataImage = renderMarkdown('![x](data:image/svg+xml;base64,AAAA)');
	assert.equal(dataImage.includes('<img'), false);
});

test('renderMarkdown keeps safe links and forces noopener+noreferrer', () => {
	const html = renderMarkdown('[safe](https://example.com/path)');
	assert.equal(html.includes('href="https://example.com/path"'), true);
	assert.equal(html.includes('target="_blank"'), true);
	assert.equal(html.includes('rel="noopener noreferrer"'), true);
});

test('renderMarkdown removes inline event handler attributes', () => {
	const html = renderMarkdown('<img src="https://example.com/a.png" onerror="alert(1)" />');
	assert.equal(html.includes('<img'), false);
	assert.equal(html.includes('&lt;img'), true);
});

test('renderMarkdown escapes attribute values emitted by markdown renderer', () => {
	const html = renderMarkdown('![x" onerror="alert(1)](https://example.com/a.png)');
	assert.equal(html.includes('" onerror='), false);
	assert.equal(html.includes('&quot; onerror=&quot;alert(1)'), true);
	assert.equal(html.includes('alt="x&quot; onerror=&quot;alert(1)"'), true);
});
