#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const repoRoot = process.cwd();

const checks = [
	{
		file: '.githooks/pre-commit',
		requiredSnippets: [
			'ERROR: mise is required for pre-commit validation.',
			'scripts/validate/desktop-build-preflight.mjs --mode local',
			'run_node_check()',
			'scripts/check-validation-hooks.mjs',
			'scripts/check-doc-portability.mjs',
			'scripts/verify-content-contract.mjs',
		],
	},
	{
		file: '.githooks/pre-push',
		requiredSnippets: [
			'ERROR: mise is required for pre-push validation.',
			'No frontend-related changes vs upstream; skipping frontend checks',
			'Skipping npm ci for',
			'.opensession/.cache/pre-push',
		],
	},
];

let hasError = false;

for (const check of checks) {
	const filePath = path.join(repoRoot, check.file);
	if (!fs.existsSync(filePath)) {
		console.error(`[validation-hooks-check] Missing file: ${check.file}`);
		hasError = true;
		continue;
	}

	const content = fs.readFileSync(filePath, 'utf8');
	for (const snippet of check.requiredSnippets) {
		if (!content.includes(snippet)) {
			console.error(
				`[validation-hooks-check] Missing required snippet in ${check.file}: ${snippet}`,
			);
			hasError = true;
		}
	}
}

if (hasError) {
	process.exit(1);
}

console.log('[validation-hooks-check] OK');
