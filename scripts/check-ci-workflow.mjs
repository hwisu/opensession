#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const repoRoot = process.cwd();
const leanWorkflowPath = path.join(repoRoot, '.github/workflows/ci.yml');
const deepWorkflowPath = path.join(repoRoot, '.github/workflows/ci-deep.yml');

function fail(message) {
	console.error(`[ci-workflow-check] ${message}`);
	process.exitCode = 1;
}

function readFileSafe(absPath) {
	try {
		return fs.readFileSync(absPath, 'utf8');
	} catch (err) {
		fail(`Cannot read ${path.relative(repoRoot, absPath)}: ${err.message}`);
		return '';
	}
}

function requireIncludes(content, snippet, message) {
	if (!content.includes(snippet)) {
		fail(message);
	}
}

function requireExcludes(content, snippet, message) {
	if (content.includes(snippet)) {
		fail(message);
	}
}

function main() {
	const leanWorkflow = readFileSafe(leanWorkflowPath);
	const deepWorkflow = readFileSafe(deepWorkflowPath);
	if (!leanWorkflow || !deepWorkflow) {
		process.exit(process.exitCode ?? 1);
	}

	requireIncludes(leanWorkflow, 'name: CI', 'Lean workflow must keep the CI badge target.');
	requireIncludes(
		leanWorkflow,
		'pull_request:\n    branches: [main]',
		'Lean workflow must run on pull requests targeting main.',
	);
	requireIncludes(
		leanWorkflow,
		'push:\n    branches: [main]',
		'Lean workflow must run on pushes to main for post-merge confirmation.',
	);
	requireIncludes(leanWorkflow, 'concurrency:', 'Lean workflow must cancel superseded runs.');
	requireIncludes(
		leanWorkflow,
		'node scripts/check-ci-workflow.mjs',
		'Lean workflow must self-validate CI split contract.',
	);
	requireIncludes(
		leanWorkflow,
		'node --test scripts/tests/*.test.mjs',
		'Lean workflow must run workflow/report script tests.',
	);
	for (const requiredJob of [
		'name: Release Surface',
		'name: Content Contract',
		'name: Format',
		'name: Clippy (${{ matrix.os }})',
		'name: Test (${{ matrix.os }})',
		'name: Worker (wasm) (${{ matrix.os }})',
		'name: Frontend (${{ matrix.os }})',
	]) {
		requireIncludes(leanWorkflow, requiredJob, `Lean workflow missing expected job: ${requiredJob}`);
	}
	for (const forbiddenJob of [
		'name: Audit',
		'name: API E2E Server (${{ matrix.os }})',
		'name: Worker + Web Live E2E (${{ matrix.os }})',
		'name: Desktop E2E (${{ matrix.os }})',
		'name: Desktop Bundle Verify (${{ matrix.os }})',
	]) {
		requireExcludes(
			leanWorkflow,
			forbiddenJob,
			`Lean workflow must not include deep validation job: ${forbiddenJob}`,
		);
	}
	requireExcludes(
		leanWorkflow,
		'github.event_name != \'pull_request\'',
		'Lean workflow must not rely on skipped non-PR jobs.',
	);
	requireExcludes(leanWorkflow, 'schedule:', 'Lean workflow must not schedule deep validation.');

	requireIncludes(deepWorkflow, 'name: CI Deep', 'Deep workflow must be defined in a separate file.');
	requireIncludes(deepWorkflow, 'workflow_dispatch:', 'Deep workflow must support manual runs.');
	requireIncludes(deepWorkflow, 'schedule:', 'Deep workflow must support scheduled runs.');
	requireExcludes(
		deepWorkflow,
		'pull_request:',
		'Deep workflow must stay off pull_request to keep PR checks lean.',
	);
	requireIncludes(deepWorkflow, 'concurrency:', 'Deep workflow must cancel superseded runs.');
	for (const requiredJob of [
		'name: Audit',
		'name: API E2E Server (${{ matrix.os }})',
		'name: Worker + Web Live E2E (${{ matrix.os }})',
		'name: Desktop E2E (${{ matrix.os }})',
		'name: Desktop Bundle Verify (${{ matrix.os }})',
	]) {
		requireIncludes(deepWorkflow, requiredJob, `Deep workflow missing expected job: ${requiredJob}`);
	}
	requireIncludes(
		deepWorkflow,
		'~/.cache/ms-playwright',
		'Deep workflow must cache Playwright browsers for live web validation.',
	);
	for (const rustCacheKey of [
		'worker-web-live-e2e-${{ matrix.os }}',
		'desktop-e2e-${{ matrix.os }}',
		'desktop-bundle-verify-${{ matrix.os }}',
	]) {
		requireIncludes(
			deepWorkflow,
			rustCacheKey,
			`Deep workflow must use rust-cache for ${rustCacheKey}.`,
		);
	}

	if (process.exitCode && process.exitCode !== 0) {
		process.exit(process.exitCode);
	}
	console.log('[ci-workflow-check] OK');
}

main();
