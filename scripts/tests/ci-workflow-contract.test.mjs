import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';

const repoRoot = process.cwd();

test('lean CI keeps PR checks small and defers heavy validation', () => {
	const leanWorkflow = fs.readFileSync(path.join(repoRoot, '.github/workflows/ci.yml'), 'utf8');

	assert.match(leanWorkflow, /pull_request:\n\s+branches: \[main\]/);
	assert.match(leanWorkflow, /push:\n\s+branches: \[main\]/);
	assert.match(leanWorkflow, /node scripts\/check-ci-workflow\.mjs/);
	assert.match(leanWorkflow, /node --test scripts\/tests\/\*\.test\.mjs/);
	assert.doesNotMatch(leanWorkflow, /name: Audit/);
	assert.doesNotMatch(leanWorkflow, /name: API E2E Server \(\$\{\{ matrix\.os \}\}\)/);
	assert.doesNotMatch(leanWorkflow, /name: Worker \+ Web Live E2E \(\$\{\{ matrix\.os \}\}\)/);
	assert.doesNotMatch(leanWorkflow, /name: Desktop E2E \(\$\{\{ matrix\.os \}\}\)/);
	assert.doesNotMatch(leanWorkflow, /name: Desktop Bundle Verify \(\$\{\{ matrix\.os \}\}\)/);
});

test('deep CI owns scheduled heavy validation and caches expensive dependencies', () => {
	const deepWorkflow = fs.readFileSync(
		path.join(repoRoot, '.github/workflows/ci-deep.yml'),
		'utf8',
	);

	assert.match(deepWorkflow, /workflow_dispatch:/);
	assert.match(deepWorkflow, /schedule:/);
	assert.doesNotMatch(deepWorkflow, /pull_request:/);
	assert.match(deepWorkflow, /name: Audit/);
	assert.match(deepWorkflow, /name: API E2E Server \(\$\{\{ matrix\.os \}\}\)/);
	assert.match(deepWorkflow, /name: Worker \+ Web Live E2E \(\$\{\{ matrix\.os \}\}\)/);
	assert.match(deepWorkflow, /name: Desktop E2E \(\$\{\{ matrix\.os \}\}\)/);
	assert.match(deepWorkflow, /name: Desktop Bundle Verify \(\$\{\{ matrix\.os \}\}\)/);
	assert.match(deepWorkflow, /~\/\.cache\/ms-playwright/);
	assert.match(deepWorkflow, /worker-web-live-e2e-\$\{\{ matrix\.os \}\}/);
	assert.match(deepWorkflow, /desktop-e2e-\$\{\{ matrix\.os \}\}/);
	assert.match(deepWorkflow, /desktop-bundle-verify-\$\{\{ matrix\.os \}\}/);
});
