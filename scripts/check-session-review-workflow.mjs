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
  if (reportScript.includes('| Metric | Value |')) {
    fail('Report must not use markdown tables for overview metrics.');
  }
  if (!reportScript.includes('Files changed](') || !reportScript.includes('Commits](')) {
    fail('Report must include PR quick links to files/commits.');
  }
  if (reportScript.includes('127.0.0.1:8788/review/local/')) {
    fail('Report must not embed localhost review links in PR comments.');
  }
  if (!reportScript.includes('**Local replay:**') || !reportScript.includes('localReviewCommand(')) {
    fail('Report must include a local replay command instead of localhost links.');
  }
  if (!reportScript.includes('**Artifact storage:**')) {
    fail('Report must include artifact storage metadata.');
  }
  if (reportScript.includes('| Session ID | Tool | Files | Commits | Open | OpenSession | JSONL | Meta | Title |')) {
    fail('Report must not use markdown tables for per-session navigation.');
  }
  if (!reportScript.includes('opensessionSourceLink(')) {
    fail('Report must build opensession.io source links for web review.');
  }
  if (!reportScript.includes('https://opensession.io/src/gh/')) {
    fail('Report must include opensession.io /src/gh link template.');
  }
  if (!reportScript.includes('[web](')) {
    fail('Report must include direct web viewer links.');
  }
  if (!reportScript.includes('[jsonl](')) {
    fail('Report must include direct jsonl file links.');
  }
  if (!reportScript.includes('buildArtifactBranchName')) {
    fail('Report script must derive a dedicated artifact branch name.');
  }
  if (!reportScript.includes('DEFAULT_MAX_BUFFER = 128 * 1024 * 1024')) {
    fail('Report script must set a large git output buffer for artifact hydration.');
  }
  if (!reportScript.includes('tryRunRaw(`git show ${ledgerRef}:${session.hail_path}`)')) {
    fail('Report script must read hail artifact payload via raw git show path.');
  }
  if (!reportScript.includes('Commit trail (')) {
    fail('Report must include commit trail for direct change navigation.');
  }
  if (!reportScript.includes('#### Reviewer Quick Digest')) {
    fail('Report must include Reviewer Quick Digest block for high-signal review context.');
  }
  if (!reportScript.includes('**Comment type:**') || !reportScript.includes('**Review ID:**')) {
    fail('Report must include review metadata for comment type and review id.');
  }
  if (!reportScript.includes('**Coverage:**')) {
    fail('Report must summarize review KPIs in digest prose.');
  }
  if (!reportScript.includes('**Top areas:**')) {
    fail('Report must summarize changed areas in the quick digest.');
  }
  if (reportScript.includes('| Session | Commit | Question | Answer |')) {
    fail('Report must not use markdown tables for Q&A digest rows.');
  }
  if (!reportScript.includes('**Question:**') || !reportScript.includes('**Answer:**')) {
    fail('Report must render Q&A digest as prose with session and commit context.');
  }
  if (!reportScript.includes('primary only (auxiliary filtered)')) {
    fail('Report must describe primary-session filtering.');
  }
  if (!reportScript.includes('<details>')) {
    fail('Report must use collapsible detail sections for long lists.');
  }
  if (!reportScript.includes('collectQaDigestFromSessions')) {
    fail('Report script must derive Q&A digest rows from session payloads.');
  }
  if (!reportScript.includes('collectAreaSummary')) {
    fail('Report script must derive area summary rows from changed files.');
  }
  if (!reportScript.includes('**Updated at (UTC):**')) {
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
  if (!workflow.includes('--publish-artifacts true')) {
    fail('workflow must request artifact publication for review reports.');
  }
  if (!workflow.includes('--artifact-branch "$ARTIFACT_BRANCH"')) {
    fail('workflow must pass resolved artifact branch context to report builder.');
  }
  if (!workflow.includes('--preserve-existing-artifacts "$PERSIST_ARTIFACTS"')) {
    fail('workflow must preserve existing archive branch contents when persistent storage is enabled.');
  }
  if (!workflow.includes('Configure git author for artifact branch')) {
    fail('workflow must configure git author before publishing artifact branch.');
  }
  if (!workflow.includes('Resolve artifact storage')) {
    fail('workflow must resolve artifact storage from cleanup config.');
  }
  if (!workflow.includes("github.event.pull_request.user.type != 'Bot'")) {
    fail('workflow must skip session review automation for bot-authored PRs.');
  }
  if (!workflow.includes('session_archive_branch')) {
    fail('workflow must support repo-local session_archive_branch setting.');
  }
  if (!workflow.includes("steps.storage.outputs.persistent != 'true'")) {
    fail('cleanup must delete ephemeral artifact branches only when no archive branch is configured.');
  }
  if (!workflow.includes('Delete ephemeral artifact branch on PR close')) {
    fail('cleanup must delete ephemeral artifact branches on PR close.');
  }
  if (!workflow.includes('--publish-artifacts "$publish_artifacts"')) {
    fail('final report builder must toggle artifact publishing based on archive policy.');
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
