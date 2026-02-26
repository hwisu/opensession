#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const repoRoot = process.cwd();
const workflowPath = path.join(repoRoot, '.github/workflows/session-review.yml');
const reportScriptPath = path.join(repoRoot, 'scripts/pr_session_report.mjs');

function fail(message) {
  console.error(`[session-review-check] ${message}`);
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

function extractCleanupBlock(workflow) {
  const match = workflow.match(/\n  cleanup:\n([\s\S]*?)(?:\n  [a-zA-Z0-9_-]+:\n|$)/);
  return match ? match[1] : '';
}

function main() {
  const workflow = readFileSafe(workflowPath);
  const reportScript = readFileSafe(reportScriptPath);
  if (!workflow || !reportScript) {
    process.exit(process.exitCode ?? 1);
  }

  if (!workflow.includes("const marker = '<!-- opensession-session-review -->';")) {
    fail('Sticky comment marker is missing or changed in session-review workflow.');
  }

  if (!reportScript.includes("'<!-- opensession-session-review -->'")) {
    fail('Update marker is missing in scripts/pr_session_report.mjs.');
  }
  if (!reportScript.includes("'<!-- opensession-session-review-final -->'")) {
    fail('Final marker is missing in scripts/pr_session_report.mjs.');
  }
  if (!reportScript.includes('Quick links: [Files changed]')) {
    fail('Report must include PR quick links to files/commits.');
  }
  if (!reportScript.includes('Local review: [Open in UI]')) {
    fail('Report must include local review deep-link.');
  }
  if (!reportScript.includes('| Session ID | Commits | Open | Meta |')) {
    fail('Report must include Open column for per-session navigation.');
  }
  if (!reportScript.includes('#### Commit trail')) {
    fail('Report must include commit trail for direct change navigation.');
  }
  if (!reportScript.includes('Updated at (UTC)')) {
    fail('Report must include update timestamp for per-run freshness.');
  }

  const cleanupBlock = extractCleanupBlock(workflow);
  if (!cleanupBlock) {
    fail('cleanup job block not found in session-review workflow.');
    process.exit(process.exitCode ?? 1);
  }

  if (!cleanupBlock.includes('permissions:')) {
    fail('cleanup job must declare explicit permissions.');
  }
  if (!cleanupBlock.includes('contents: write')) {
    fail('cleanup job must include permissions.contents=write for ref deletion.');
  }
  if (!cleanupBlock.includes('issues: write')) {
    fail('cleanup job must include permissions.issues=write for final comment.');
  }
  if (!workflow.includes('issues: write')) {
    fail('workflow must include issues: write for sticky comment upsert.');
  }
  if (!workflow.includes('--repo "$REPO_FULL_NAME"')) {
    fail('workflow must pass repository context to report builder.');
  }
  if (!workflow.includes('--pr-number "$PR_NUMBER"')) {
    fail('workflow must pass PR number context to report builder.');
  }
  if (!workflow.includes('github.rest.issues.deleteComment')) {
    fail('sticky comment upsert must dedupe stale marker comments.');
  }

  if (process.exitCode && process.exitCode !== 0) {
    process.exit(process.exitCode);
  }
  console.log('[session-review-check] OK');
}

main();
