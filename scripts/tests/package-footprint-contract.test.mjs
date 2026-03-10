import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';

const repoRoot = process.cwd();

function readJson(relativePath) {
	return JSON.parse(fs.readFileSync(path.join(repoRoot, relativePath), 'utf8'));
}

test('web avoids direct markdown/highlight dependencies already owned by @opensession/ui', () => {
	const webPackage = readJson('web/package.json');
	const dependencies = webPackage.dependencies ?? {};

	assert.equal(typeof dependencies['@opensession/ui'], 'string');
	assert.equal('highlight.js' in dependencies, false);
	assert.equal('marked' in dependencies, false);
});

test('ui markdown renderer does not depend on server-side dom shims', () => {
	const uiPackage = readJson('packages/ui/package.json');
	const dependencies = uiPackage.dependencies ?? {};

	assert.equal('isomorphic-dompurify' in dependencies, false);
});
