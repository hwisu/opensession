#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const repoRoot = process.cwd();

const docTargets = [
	'README.md',
	'README.ko.md',
	'CONTRIBUTING.md',
	'docs.md',
	'docs/development-validation-flow.md',
	'web/README.md',
	'docs/harness-auto-improve-loop.md',
];

const absolutePathPatterns = [
	{ name: 'macOS absolute path', regex: /\/Users\/[^\s`)"']+/g },
	{ name: 'Linux absolute path', regex: /\/home\/[^\s`)"']+/g },
	{ name: 'Windows absolute path', regex: /[A-Za-z]:\\[^\s`)"']+/g },
];

let hasError = false;

for (const relPath of docTargets) {
	const absPath = path.join(repoRoot, relPath);
	if (!fs.existsSync(absPath)) {
		console.error(`[doc-portability-check] Missing file: ${relPath}`);
		hasError = true;
		continue;
	}

	const text = fs.readFileSync(absPath, 'utf8');
	const lines = text.split(/\r?\n/);
	lines.forEach((line, idx) => {
		for (const pattern of absolutePathPatterns) {
			pattern.regex.lastIndex = 0;
			const match = pattern.regex.exec(line);
			if (!match) continue;
			console.error(
				`[doc-portability-check] ${relPath}:${idx + 1} contains ${pattern.name}: ${match[0]}`,
			);
			hasError = true;
		}
	});
}

if (hasError) {
	process.exit(1);
}

console.log('[doc-portability-check] OK');
