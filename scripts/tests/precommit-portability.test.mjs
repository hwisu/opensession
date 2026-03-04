import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import path from 'node:path';

// @covers compat.precommit.hash_command_fallback

test('pre-commit lock hash uses sha256sum/shasum/openssl fallback chain', () => {
	const repoRoot = process.cwd();
	const hookPath = path.join(repoRoot, '.githooks', 'pre-commit');
	const content = fs.readFileSync(hookPath, 'utf8');

	assert.match(content, /hash_file_sha256\(\)/);
	assert.match(content, /command -v sha256sum/);
	assert.match(content, /command -v shasum/);
	assert.match(content, /command -v openssl/);
	assert.match(content, /LOCK_HASH_BEFORE=\$\(hash_file_sha256 Cargo\.lock\)/);
	assert.match(content, /LOCK_HASH_AFTER=\$\(hash_file_sha256 Cargo\.lock\)/);
});
