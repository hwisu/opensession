#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const repoRoot = process.cwd();

function readRequiredFile(relativePath) {
	const filePath = path.join(repoRoot, relativePath);
	if (!fs.existsSync(filePath)) {
		console.error(`[rust-artifact-guardrails] Missing file: ${relativePath}`);
		process.exit(1);
	}

	return fs.readFileSync(filePath, 'utf8');
}

function requireRegex(content, regex, description) {
	if (!regex.test(content)) {
		console.error(`[rust-artifact-guardrails] Missing ${description}`);
		process.exit(1);
	}
}

const cargoToml = readRequiredFile('Cargo.toml');
requireRegex(
	cargoToml,
	/\[profile\.dev\][\s\S]*?incremental\s*=\s*false/,
	'`[profile.dev] incremental = false` in Cargo.toml',
);
requireRegex(
	cargoToml,
	/\[profile\.test\][\s\S]*?incremental\s*=\s*false/,
	'`[profile.test] incremental = false` in Cargo.toml',
);

const buildSh = readRequiredFile('build.sh');
for (const snippet of [
	'prune_rust_build_artifacts()',
	'rm -rf target/debug/incremental',
	'target-rustup-*/debug/incremental',
	'target-rustup-*',
]) {
	if (!buildSh.includes(snippet)) {
		console.error(
			`[rust-artifact-guardrails] Missing required build.sh snippet: ${snippet}`,
		);
		process.exit(1);
	}
}

console.log('[rust-artifact-guardrails] OK');
