#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const repoRoot = process.cwd();
const args = new Set(process.argv.slice(2));
const writeMode = args.has('--write');
const printMode = args.has('--print');

const targets = [
	{ kind: 'toml', file: 'desktop/src-tauri/Cargo.toml', section: 'package' },
	{ kind: 'json', file: 'desktop/src-tauri/tauri.conf.json' },
	{ kind: 'json', file: 'desktop/package.json' },
	{ kind: 'package-lock', file: 'desktop/package-lock.json' },
];

function readFile(relPath) {
	const absPath = path.join(repoRoot, relPath);
	if (!fs.existsSync(absPath)) {
		throw new Error(`missing file: ${relPath}`);
	}
	return fs.readFileSync(absPath, 'utf8');
}

function writeFile(relPath, content) {
	const absPath = path.join(repoRoot, relPath);
	fs.writeFileSync(absPath, content);
}

function parseWorkspaceVersion() {
	const cargoToml = readFile('Cargo.toml');
	const section = cargoToml.match(/\[workspace\.package\][\s\S]*?(?=^\[[^\]]+\]|\Z)/m);
	if (!section) throw new Error('missing [workspace.package] section in Cargo.toml');
	const version = section[0].match(/^\s*version\s*=\s*"([^"]+)"\s*$/m);
	if (!version) throw new Error('missing workspace package version in Cargo.toml');
	return version[1];
}

function parseTomlSectionVersion(content, sectionName) {
	const sectionPattern = new RegExp(
		`\\[${sectionName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}\\][\\s\\S]*?(?=^\\[[^\\]]+\\]|\\Z)`,
		'm',
	);
	const section = content.match(sectionPattern);
	if (!section) throw new Error(`missing [${sectionName}] section`);
	const version = section[0].match(/^\s*version\s*=\s*"([^"]+)"\s*$/m);
	if (!version) throw new Error(`missing version in [${sectionName}] section`);
	return version[1];
}

function replaceTomlSectionVersion(content, sectionName, nextVersion) {
	const escaped = sectionName.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
	const sectionPattern = new RegExp(`(\\[${escaped}\\][\\s\\S]*?)(?=^\\[[^\\]]+\\]|\\Z)`, 'm');
	const section = content.match(sectionPattern);
	if (!section) throw new Error(`missing [${sectionName}] section`);
	const updatedSection = section[0].replace(
		/^\s*version\s*=\s*"[^"]*"\s*$/m,
		`version = "${nextVersion}"`,
	);
	if (updatedSection === section[0]) {
		throw new Error(`failed to update version in [${sectionName}] section`);
	}
	return content.replace(sectionPattern, updatedSection);
}

function checkAndMaybeUpdate() {
	const expectedVersion = parseWorkspaceVersion();
	if (printMode) {
		console.log(expectedVersion);
		return { expectedVersion, mismatches: [] };
	}

	const mismatches = [];
	for (const target of targets) {
		if (target.kind === 'toml') {
			const source = readFile(target.file);
			const current = parseTomlSectionVersion(source, target.section);
			if (current !== expectedVersion) {
				mismatches.push({ file: target.file, current, expected: expectedVersion });
				if (writeMode) {
					const updated = replaceTomlSectionVersion(source, target.section, expectedVersion);
					writeFile(target.file, updated);
				}
			}
			continue;
		}

		if (target.kind === 'json') {
			const parsed = JSON.parse(readFile(target.file));
			const current = String(parsed.version ?? '');
			if (current !== expectedVersion) {
				mismatches.push({ file: target.file, current, expected: expectedVersion });
				if (writeMode) {
					parsed.version = expectedVersion;
					writeFile(target.file, `${JSON.stringify(parsed, null, '\t')}\n`);
				}
			}
			continue;
		}

		if (target.kind === 'package-lock') {
			const parsed = JSON.parse(readFile(target.file));
			const current = String(parsed.version ?? '');
			const currentRoot = String(parsed.packages?.['']?.version ?? '');
			const mismatch = current !== expectedVersion || currentRoot !== expectedVersion;
			if (mismatch) {
				mismatches.push({
					file: target.file,
					current: `${current} (root:${currentRoot})`,
					expected: expectedVersion,
				});
				if (writeMode) {
					parsed.version = expectedVersion;
					if (!parsed.packages || typeof parsed.packages !== 'object') {
						parsed.packages = {};
					}
					if (!parsed.packages[''] || typeof parsed.packages[''] !== 'object') {
						parsed.packages[''] = {};
					}
					parsed.packages[''].version = expectedVersion;
					writeFile(target.file, `${JSON.stringify(parsed, null, '\t')}\n`);
				}
			}
		}
	}

	return { expectedVersion, mismatches };
}

try {
	const { expectedVersion, mismatches } = checkAndMaybeUpdate();
	if (printMode) process.exit(0);

	if (!writeMode && mismatches.length > 0) {
		console.error('[product-version-sync] mismatch detected');
		for (const mismatch of mismatches) {
			console.error(
				`  - ${mismatch.file}: found "${mismatch.current}", expected "${mismatch.expected}"`,
			);
		}
		console.error('Run: node scripts/sync-product-version.mjs --write');
		process.exit(1);
	}

	if (writeMode && mismatches.length > 0) {
		for (const mismatch of mismatches) {
			console.log(
				`[product-version-sync] updated ${mismatch.file}: ${mismatch.current} -> ${mismatch.expected}`,
			);
		}
	} else {
		console.log(`[product-version-sync] OK (version ${expectedVersion})`);
	}
} catch (error) {
	const message = error instanceof Error ? error.message : String(error);
	console.error(`[product-version-sync] ${message}`);
	process.exit(1);
}
