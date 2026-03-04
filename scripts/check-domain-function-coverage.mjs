#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const repoRoot = process.cwd();
const policyPath = path.join(repoRoot, 'docs', 'domain-function-coverage.json');
const MARKER_RE = /@coversfn\s+([A-Za-z0-9_.-]+)\s+(success|error|edge)\b/g;
const RUST_FN_RE =
	/(^|\n)\s*pub\s+(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(([\s\S]*?)\)\s*(?:->\s*([\s\S]*?))?\s*\{/g;

function fail(message) {
	console.error(`[domain-coverage] ${message}`);
}

function loadPolicy() {
	if (!fs.existsSync(policyPath)) {
		fail(`Missing policy file: ${path.relative(repoRoot, policyPath)}`);
		process.exit(1);
	}

	let parsed;
	try {
		parsed = JSON.parse(fs.readFileSync(policyPath, 'utf8'));
	} catch (error) {
		fail(`Failed to parse policy JSON: ${(error && error.message) || error}`);
		process.exit(1);
	}

	if (!parsed || !Array.isArray(parsed.targets) || parsed.targets.length === 0) {
		fail('Policy must include a non-empty `targets` array.');
		process.exit(1);
	}
	if (!Array.isArray(parsed.test_roots) || parsed.test_roots.length === 0) {
		fail('Policy must include a non-empty `test_roots` array.');
		process.exit(1);
	}
	return parsed;
}

function listRustFiles(baseDir) {
	if (!fs.existsSync(baseDir)) return [];
	const out = [];
	const stack = [baseDir];
	while (stack.length > 0) {
		const current = stack.pop();
		if (!current) continue;
		const entries = fs.readdirSync(current, { withFileTypes: true });
		for (const entry of entries) {
			const absPath = path.join(current, entry.name);
			if (entry.isDirectory()) {
				stack.push(absPath);
				continue;
			}
			if (!entry.isFile() || !entry.name.endsWith('.rs')) continue;
			out.push(absPath);
		}
	}
	return out;
}

function isResultType(returnType) {
	if (!returnType) return false;
	return /\bResult\s*</.test(returnType.replace(/\s+/g, ' '));
}

function moduleNameFromPath(filePath) {
	const base = path.basename(filePath);
	return base.endsWith('.rs') ? base.slice(0, -3) : base;
}

function extractDomainFunctions(targetRelPath) {
	const absPath = path.join(repoRoot, targetRelPath);
	if (!fs.existsSync(absPath)) {
		fail(`Target file not found: ${targetRelPath}`);
		process.exit(1);
	}

	const content = fs.readFileSync(absPath, 'utf8');
	const moduleName = moduleNameFromPath(targetRelPath);
	const rows = [];

	RUST_FN_RE.lastIndex = 0;
	let match;
	while ((match = RUST_FN_RE.exec(content)) !== null) {
		const fnName = match[2];
		const returnType = match[4] ?? '';
		const requiredKinds = isResultType(returnType) ? ['success', 'error'] : ['success'];
		const line = content.slice(0, match.index).split('\n').length;
		rows.push({
			id: `${moduleName}.${fnName}`,
			fnName,
			moduleName,
			file: targetRelPath,
			line,
			requiredKinds,
		});
	}
	return rows;
}

function collectMarkers(testRoots) {
	const markerMap = new Map();
	for (const root of testRoots) {
		const absRoot = path.join(repoRoot, root);
		for (const filePath of listRustFiles(absRoot)) {
			const relPath = path.relative(repoRoot, filePath);
			const content = fs.readFileSync(filePath, 'utf8');
			MARKER_RE.lastIndex = 0;
			let match;
			while ((match = MARKER_RE.exec(content)) !== null) {
				const id = match[1];
				const kind = match[2];
				const line = content.slice(0, match.index).split('\n').length;
				const current =
					markerMap.get(id) ??
					{
						kinds: new Set(),
						locations: [],
					};
				current.kinds.add(kind);
				current.locations.push(`${relPath}:${line}`);
				markerMap.set(id, current);
			}
		}
	}
	return markerMap;
}

function main() {
	const policy = loadPolicy();
	const domainFunctions = policy.targets.flatMap((target) => extractDomainFunctions(target));
	const functionById = new Map(domainFunctions.map((row) => [row.id, row]));
	const markerMap = collectMarkers(policy.test_roots);

	let hasError = false;

	for (const fnRow of domainFunctions) {
		const marker = markerMap.get(fnRow.id);
		if (!marker) {
			fail(
				`Missing @coversfn marker for ${fnRow.id} (${fnRow.file}:${fnRow.line}). Required kinds: ${fnRow.requiredKinds.join(', ')}`,
			);
			hasError = true;
			continue;
		}

		for (const kind of fnRow.requiredKinds) {
			if (!marker.kinds.has(kind)) {
				fail(
					`Missing @coversfn ${fnRow.id} ${kind}. Found kinds: ${Array.from(marker.kinds).join(', ')} (${marker.locations.join(', ')})`,
				);
				hasError = true;
			}
		}
	}

	for (const [id, marker] of markerMap.entries()) {
		if (!functionById.has(id)) {
			fail(`Unknown @coversfn target '${id}' at ${marker.locations.join(', ')}`);
			hasError = true;
		}
	}

	if (hasError) {
		process.exit(1);
	}

	console.log(
		`[domain-coverage] OK (${domainFunctions.length} domain functions, ${markerMap.size} marker targets)`,
	);
}

main();
