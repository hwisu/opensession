#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const repoRoot = process.cwd();
const matrixPath = path.join(repoRoot, 'docs', 'e2e-functional-matrix.yaml');
const COVER_RE = /@covers\s+([A-Za-z0-9._-]+)/g;

const scanRoots = [
	'crates',
	path.join('web', 'e2e'),
	path.join('web', 'e2e-live'),
	'desktop',
	path.join('scripts', 'tests'),
];

const ignoreDirNames = new Set([
	'.git',
	'node_modules',
	'target',
	'build',
	'.next',
	'.svelte-kit',
	'.wrangler',
	'.opensession',
	'coverage',
	'dist',
	'out',
]);

const allowedExt = new Set(['.rs', '.ts', '.tsx', '.js', '.mjs', '.cjs', '.sh']);

function fail(message) {
	console.error(`[functional-coverage] ${message}`);
}

function readMatrixRows() {
	if (!fs.existsSync(matrixPath)) {
		fail(`Missing matrix file: ${path.relative(repoRoot, matrixPath)}`);
		process.exit(1);
	}

	let parsed;
	try {
		parsed = JSON.parse(fs.readFileSync(matrixPath, 'utf8'));
	} catch (error) {
		fail(`Matrix file is not valid JSON/YAML-JSON: ${(error && error.message) || error}`);
		process.exit(1);
	}

	if (!parsed || !Array.isArray(parsed.rows)) {
		fail('Matrix must be an object with a `rows` array.');
		process.exit(1);
	}

	const seen = new Set();
	const rows = [];
	let hasError = false;

	for (const [index, row] of parsed.rows.entries()) {
		const location = `rows[${index}]`;
		const required = ['surface', 'runtime', 'path_or_flow', 'scenario', 'test_id', 'os'];
		for (const key of required) {
			if (!(key in row)) {
				fail(`Missing required field '${key}' in ${location}`);
				hasError = true;
			}
		}
		if (typeof row.test_id !== 'string' || row.test_id.trim() === '') {
			fail(`Invalid test_id in ${location}`);
			hasError = true;
			continue;
		}
		if (seen.has(row.test_id)) {
			fail(`Duplicate test_id in matrix: ${row.test_id}`);
			hasError = true;
		}
		seen.add(row.test_id);
		if (!Array.isArray(row.os) || row.os.length === 0) {
			fail(`Field 'os' must be a non-empty array in ${location} (${row.test_id})`);
			hasError = true;
		}
		rows.push(row);
	}

	if (hasError) {
		process.exit(1);
	}

	return rows;
}

function walkFiles(dir, out) {
	if (!fs.existsSync(dir)) return;
	const entries = fs.readdirSync(dir, { withFileTypes: true });
	for (const entry of entries) {
		const absPath = path.join(dir, entry.name);
		if (entry.isDirectory()) {
			if (ignoreDirNames.has(entry.name)) continue;
			walkFiles(absPath, out);
			continue;
		}
		if (!entry.isFile()) continue;
		const ext = path.extname(entry.name);
		if (!allowedExt.has(ext)) continue;
		out.push(absPath);
	}
}

function collectCovers() {
	const files = [];
	for (const root of scanRoots) {
		walkFiles(path.join(repoRoot, root), files);
	}

	const coverMap = new Map();
	for (const file of files) {
		const content = fs.readFileSync(file, 'utf8');
		COVER_RE.lastIndex = 0;
		let match;
		while ((match = COVER_RE.exec(content)) !== null) {
			const id = match[1];
			const location = path.relative(repoRoot, file);
			const locations = coverMap.get(id) ?? [];
			locations.push(location);
			coverMap.set(id, locations);
		}
	}
	return coverMap;
}

function main() {
	const rows = readMatrixRows();
	const matrixIds = new Set(rows.map((row) => row.test_id));
	const coverMap = collectCovers();
	let hasError = false;

	for (const id of matrixIds) {
		const locations = coverMap.get(id) ?? [];
		if (locations.length === 0) {
			fail(`Matrix test_id is missing @covers mapping: ${id}`);
			hasError = true;
			continue;
		}
		if (locations.length > 1) {
			fail(`test_id must map to exactly one test (@covers duplicated): ${id} -> ${locations.join(', ')}`);
			hasError = true;
		}
	}

	for (const [id, locations] of coverMap.entries()) {
		if (!matrixIds.has(id)) {
			fail(`@covers references unknown test_id not present in matrix: ${id} (${locations.join(', ')})`);
			hasError = true;
		}
	}

	if (hasError) {
		process.exit(1);
	}

	console.log(`[functional-coverage] OK (${rows.length} matrix rows, ${coverMap.size} @covers ids)`);
}

main();
