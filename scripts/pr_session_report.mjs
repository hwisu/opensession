#!/usr/bin/env node

import { execFileSync, execSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const DEFAULT_MAX_BUFFER = 128 * 1024 * 1024;

function runRaw(cmd, options = {}) {
  const { maxBuffer = DEFAULT_MAX_BUFFER } = options;
  return execSync(cmd, {
    stdio: ['ignore', 'pipe', 'pipe'],
    encoding: 'utf8',
    maxBuffer,
  });
}

function run(cmd, options = {}) {
  return runRaw(cmd, options).trim();
}

function tryRun(cmd) {
  try {
    return run(cmd);
  } catch {
    return '';
  }
}

function tryRunRaw(cmd, options = {}) {
  try {
    return runRaw(cmd, options);
  } catch {
    return '';
  }
}

function gitCommandSucceeds(cmd, options = {}) {
  try {
    runRaw(cmd, options);
    return true;
  } catch {
    return false;
  }
}

function parseArgs(argv) {
  const args = {};
  for (let i = 2; i < argv.length; i += 1) {
    const token = argv[i];
    if (!token.startsWith('--')) continue;
    const key = token.slice(2);
    const value = argv[i + 1] && !argv[i + 1].startsWith('--') ? argv[++i] : 'true';
    args[key] = value;
  }
  return args;
}

function unique(items) {
  return Array.from(new Set(items));
}

function runGit(args, options = {}) {
  const { cwd = process.cwd(), allowFail = false, maxBuffer = DEFAULT_MAX_BUFFER } = options;
  try {
    return execFileSync('git', args, {
      cwd,
      stdio: ['ignore', 'pipe', 'pipe'],
      encoding: 'utf8',
      maxBuffer,
    }).trim();
  } catch (error) {
    if (allowFail) return '';
    const stderr = error?.stderr ? String(error.stderr).trim() : '';
    const suffix = stderr ? `: ${stderr}` : '';
    throw new Error(`git ${args.join(' ')} failed${suffix}`);
  }
}

function sanitizeReviewIdComponent(value) {
  return String(value)
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');
}

function buildReviewId(repoFullName, prNumber, head) {
  if (!repoFullName || !prNumber || !head) return null;
  const [ownerRaw, repoRaw] = String(repoFullName).split('/');
  if (!ownerRaw || !repoRaw) return null;
  const owner = sanitizeReviewIdComponent(ownerRaw);
  const repo = sanitizeReviewIdComponent(repoRaw);
  if (!owner || !repo) return null;
  return `gh-${owner}-${repo}-pr${prNumber}-${shortSha(head)}`;
}

function buildArtifactBranchName(prNumber) {
  if (!prNumber) return null;
  return `opensession/pr-${prNumber}-sessions`;
}

function buildArtifactRoot(reviewId) {
  if (!reviewId) return null;
  return `reviews/${reviewId}`;
}

function localReviewLink(reviewId, sessionId = '', commitSha = '') {
  if (!reviewId) return null;
  const url = new URL(`http://127.0.0.1:8788/review/local/${reviewId}`);
  if (sessionId) url.searchParams.set('session', sessionId);
  if (commitSha) url.searchParams.set('commit', commitSha);
  return url.toString();
}

function githubBlobLink(repoFullName, branchName, filePath) {
  if (!repoFullName || !branchName || !filePath) return null;
  return `https://github.com/${repoFullName}/blob/${branchName}/${filePath}`;
}

function githubTreeLink(repoFullName, branchName, filePath = '') {
  if (!repoFullName || !branchName) return null;
  if (!filePath) return `https://github.com/${repoFullName}/tree/${branchName}`;
  return `https://github.com/${repoFullName}/tree/${branchName}/${filePath}`;
}

function encodeRepoPath(pathValue) {
  return String(pathValue)
    .split('/')
    .filter((segment) => segment.length > 0)
    .map((segment) => encodeURIComponent(segment))
    .join('/');
}

function opensessionSourceLink(repoFullName, rRef, repoPath) {
  if (!repoFullName || !rRef || !repoPath) return null;
  const [owner, repo] = String(repoFullName).split('/');
  if (!owner || !repo) return null;
  const refSegment = encodeURIComponent(String(rRef));
  const pathSegment = encodeRepoPath(repoPath);
  if (!pathSegment) return null;
  return `https://opensession.io/src/gh/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}/ref/${refSegment}/path/${pathSegment}`;
}

function shortSha(sha) {
  return String(sha).slice(0, 7);
}

function commitLink(repoFullName, sha) {
  if (!repoFullName || !sha) return `\`${shortSha(sha)}\``;
  return `[\`${shortSha(sha)}\`](https://github.com/${repoFullName}/commit/${sha})`;
}

function pullRequestLinks(repoFullName, prNumber, base, head) {
  if (!repoFullName || !prNumber) return null;
  const root = `https://github.com/${repoFullName}/pull/${prNumber}`;
  const links = {
    files: `${root}/files`,
    commits: `${root}/commits`,
  };
  if (base && head) {
    links.compare = `https://github.com/${repoFullName}/compare/${base}...${head}`;
  }
  return links;
}

function collectCommitRange(base, head) {
  if (!head) return [];
  if (!base) return [head];
  const out = tryRun(`git rev-list --ancestry-path ${base}..${head}`);
  if (!out) return [head];
  return out.split('\n').map((line) => line.trim()).filter(Boolean);
}

function parseJson(raw) {
  try {
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

function readJsonAtRef(ledgerRef, relPath) {
  if (!ledgerRef || !relPath) return null;
  const raw = tryRunRaw(`git show ${ledgerRef}:${relPath}`);
  if (!raw) return null;
  return parseJson(raw);
}

function normalizeSessionTitle(value, maxLen = 96) {
  return normalizeDigestText(value, maxLen);
}

function collectIndexedSessions(ledgerRef, commits) {
  const bySession = new Map();
  for (const sha of commits) {
    const prefix = `v1/index/commits/${sha}`;
    const listing = tryRun(`git ls-tree -r --name-only ${ledgerRef} -- ${prefix}`);
    if (!listing) continue;

    for (const relPath of listing.split('\n').map((line) => line.trim()).filter(Boolean)) {
      const raw = tryRun(`git show ${ledgerRef}:${relPath}`);
      if (!raw) continue;
      const payload = parseJson(raw);
      if (!payload) continue;
      const sessionId = payload.session_id;
      if (!sessionId) continue;
      const meta = readJsonAtRef(ledgerRef, payload.meta_path);
      const sessionRole = String(meta?.session_role ?? '').trim().toLowerCase();
      if (sessionRole === 'auxiliary') continue;
      const existing = bySession.get(sessionId) ?? {
        session_id: sessionId,
        commits: [],
        hail_path: payload.hail_path,
        meta_path: payload.meta_path,
        tool: String(meta?.tool ?? '').trim(),
        session_role: sessionRole || 'primary',
        title: normalizeSessionTitle(meta?.title),
        files_changed: Number.isFinite(meta?.stats?.files_changed) ? meta.stats.files_changed : 0,
      };
      existing.commits = unique([...existing.commits, sha]);
      if (!existing.tool && meta?.tool) existing.tool = String(meta.tool).trim();
      if ((!existing.title || existing.title === '-') && meta?.title) {
        existing.title = normalizeSessionTitle(meta.title);
      }
      if (!existing.files_changed && Number.isFinite(meta?.stats?.files_changed)) {
        existing.files_changed = meta.stats.files_changed;
      }
      bySession.set(sessionId, existing);
    }
  }
  return Array.from(bySession.values()).sort((a, b) =>
    (b.files_changed - a.files_changed)
      || (b.commits.length - a.commits.length)
      || a.session_id.localeCompare(b.session_id));
}

function isTestFilePath(filePath) {
  const normalized = String(filePath || '')
    .trim()
    .replace(/\\/g, '/')
    .toLowerCase();
  if (!normalized) return false;
  return (
    normalized.includes('/tests/') ||
    normalized.includes('/test/') ||
    normalized.includes('/__tests__/') ||
    normalized.endsWith('.test.ts') ||
    normalized.endsWith('.test.tsx') ||
    normalized.endsWith('.test.js') ||
    normalized.endsWith('.test.jsx') ||
    normalized.endsWith('.spec.ts') ||
    normalized.endsWith('.spec.tsx') ||
    normalized.endsWith('.spec.js') ||
    normalized.endsWith('.spec.jsx') ||
    normalized.endsWith('_test.rs') ||
    normalized.endsWith('_spec.rs') ||
    normalized.endsWith('_test.py')
  );
}

function collectChangedFiles(base, head) {
  if (base && head) {
    const out = tryRun(`git diff --name-only ${base}..${head}`);
    if (out) return unique(out.split('\n').map((line) => line.trim()).filter(Boolean)).sort();
  }
  if (head) {
    const out = tryRun(`git show --name-only --pretty=format: ${head}`);
    if (out) return unique(out.split('\n').map((line) => line.trim()).filter(Boolean)).sort();
  }
  return [];
}

function normalizeDigestText(value, maxLen = 220) {
  const compact = String(value ?? '').replace(/\s+/g, ' ').trim();
  if (!compact) return '';
  if (compact.length <= maxLen) return compact;
  return `${compact.slice(0, maxLen - 1)}…`;
}

function extractTextFromEventContent(content) {
  if (!content || !Array.isArray(content.blocks)) return '';
  for (const block of content.blocks) {
    if (block?.type !== 'Text') continue;
    const normalized = normalizeDigestText(block.text);
    if (normalized) return normalized;
  }
  return '';
}

function extractEventRecord(parsedLine) {
  if (parsedLine && parsedLine.event_type && parsedLine.content) return parsedLine;
  if (parsedLine?.data?.event_type && parsedLine?.data?.content) return parsedLine.data;
  if (parsedLine?.event?.event_type && parsedLine?.event?.content) return parsedLine.event;
  if (parsedLine?.type === 'event' && parsedLine?.data?.event_type && parsedLine?.data?.content) {
    return parsedLine.data;
  }
  return null;
}

function collectEventsFromSessionRaw(raw) {
  const text = String(raw ?? '').trim();
  if (!text) return [];

  try {
    const parsed = JSON.parse(text);
    if (parsed && Array.isArray(parsed.events)) {
      return parsed.events.filter((event) => event && event.event_type && event.content);
    }
  } catch {
    // continue with JSONL parsing
  }

  const events = [];
  for (const line of text.split('\n')) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    try {
      const parsed = JSON.parse(trimmed);
      const event = extractEventRecord(parsed);
      if (event) events.push(event);
    } catch {
      // ignore malformed line
    }
  }
  return events;
}

function collectQaDigestFromSessions(ledgerRef, sessions) {
  const pairs = [];
  const pendingQuestions = [];
  const seenPairs = new Set();

  for (const session of sessions) {
    if (pairs.length >= 6) break;
    if (!session.hail_path) continue;
    const raw = tryRunRaw(`git show ${ledgerRef}:${session.hail_path}`);
    if (!raw) continue;
    const events = collectEventsFromSessionRaw(raw);
    for (const event of events) {
      const source = String(event?.attributes?.source ?? '').trim().toLowerCase();
      if (source === 'interactive_question') {
        const question = extractTextFromEventContent(event.content);
        if (question) {
          pendingQuestions.push({
            question,
            session_id: session.session_id,
            commit: session.commits[0] ?? '',
          });
        }
        continue;
      }
      if (source === 'interactive') {
        const answer = extractTextFromEventContent(event.content);
        if (!answer) continue;
        const queued = pendingQuestions.shift() ?? {
          question: '(interactive question missing)',
          session_id: session.session_id,
          commit: session.commits[0] ?? '',
        };
        const pair = {
          question: queued.question,
          answer,
          session_id: session.session_id,
          commit: queued.commit || session.commits[0] || '',
        };
        const pairKey = `${pair.question}\n${pair.answer}`;
        if (!seenPairs.has(pairKey)) {
          seenPairs.add(pairKey);
          pairs.push(pair);
        }
        if (pairs.length >= 6) break;
      }
    }
  }

  for (const pending of pendingQuestions) {
    if (pairs.length >= 6) break;
    const pair = {
      question: pending.question,
      answer: null,
      session_id: pending.session_id,
      commit: pending.commit,
    };
    const pairKey = `${pair.question}\n`;
    if (!seenPairs.has(pairKey)) {
      seenPairs.add(pairKey);
      pairs.push(pair);
    }
  }

  return {
    pairs: pairs.slice(0, 6),
  };
}

function collectAreaSummary(changedFiles) {
  const counts = new Map();
  for (const rawPath of changedFiles) {
    const normalized = String(rawPath ?? '').trim().replace(/\\/g, '/');
    if (!normalized) continue;
    const [head] = normalized.split('/');
    const area = normalized.includes('/') ? head : '(root)';
    counts.set(area, (counts.get(area) ?? 0) + 1);
  }
  return Array.from(counts.entries())
    .map(([area, count]) => ({ area, count }))
    .sort((a, b) => (b.count - a.count) || a.area.localeCompare(b.area));
}

function escapeTableCell(value) {
  return String(value ?? '-')
    .replace(/\|/g, '\\|')
    .replace(/\r?\n/g, '<br>');
}

function pushDetailsList(lines, summary, items, formatter, limit = 12) {
  if (!items.length) return;
  lines.push('<details>');
  lines.push(`<summary>${summary}</summary>`);
  lines.push('');
  for (const item of items.slice(0, limit)) {
    lines.push(`- ${formatter(item)}`);
  }
  if (items.length > limit) {
    lines.push(`- ...and ${items.length - limit} more`);
  }
  lines.push('</details>');
  lines.push('');
}

function writeFileAt(worktreeDir, relPath, body) {
  const normalized = relPath.replace(/\\/g, '/').replace(/^\/+/, '');
  if (!normalized) return;
  const abs = path.join(worktreeDir, normalized);
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, body, 'utf8');
}

function ensureTrailingNewline(value) {
  if (value.length === 0) return '\n';
  return value.endsWith('\n') ? value : `${value}\n`;
}

function publishArtifactsBranch({
  enabled,
  repoFullName,
  prNumber,
  head,
  base,
  ledgerRef,
  reviewId,
  generatedAt,
  commits,
  sessions,
}) {
  const branchName = buildArtifactBranchName(prNumber);
  const artifactRoot = buildArtifactRoot(reviewId);
  if (!branchName || !artifactRoot) {
    return {
      enabled: false,
      branchName: null,
      artifactRoot: null,
      manifestPath: null,
      error: null,
      treeLink: null,
    };
  }

  const manifestPath = `${artifactRoot}/manifest.json`;
  const treeLink = githubTreeLink(repoFullName, branchName, artifactRoot);
  if (!enabled) {
    return {
      enabled: false,
      branchName,
      artifactRoot,
      manifestPath,
      error: null,
      treeLink,
    };
  }

  const tmpBase = fs.mkdtempSync(path.join(os.tmpdir(), 'opensession-pr-artifacts-'));
  const worktreeDir = path.join(tmpBase, 'worktree');
  const manifestSessions = [];
  let publishError = null;
  try {
    runGit(['worktree', 'add', '--detach', worktreeDir]);
    runGit(['checkout', '--orphan', branchName], { cwd: worktreeDir });
    runGit(['rm', '-rf', '.'], { cwd: worktreeDir, allowFail: true });

    for (const entry of fs.readdirSync(worktreeDir)) {
      if (entry === '.git') continue;
      fs.rmSync(path.join(worktreeDir, entry), { recursive: true, force: true });
    }

    for (const session of sessions) {
      const metaArtifactPath = session.meta_path
        ? `${artifactRoot}/${session.meta_path}`
        : null;
      const hailArtifactPath = session.hail_path
        ? `${artifactRoot}/${session.hail_path}`
        : null;

      if (session.meta_path) {
        const metaBody = tryRunRaw(`git show ${ledgerRef}:${session.meta_path}`);
        if (metaBody) writeFileAt(worktreeDir, metaArtifactPath, ensureTrailingNewline(metaBody));
      }
      if (session.hail_path) {
        const hailBody = tryRunRaw(`git show ${ledgerRef}:${session.hail_path}`);
        if (hailBody) writeFileAt(worktreeDir, hailArtifactPath, ensureTrailingNewline(hailBody));
      }

      manifestSessions.push({
        session_id: session.session_id,
        commits: session.commits,
        meta_path: session.meta_path ?? null,
        hail_path: session.hail_path ?? null,
        artifact_meta_path: metaArtifactPath,
        artifact_hail_path: hailArtifactPath,
      });
    }

    const manifest = {
      generated_at: generatedAt,
      repo: repoFullName,
      pr_number: prNumber,
      base_sha: base || null,
      head_sha: head || null,
      ledger_ref: ledgerRef,
      review_id: reviewId,
      branch: branchName,
      artifact_root: artifactRoot,
      commit_count: commits.length,
      session_count: sessions.length,
      sessions: manifestSessions,
    };

    writeFileAt(worktreeDir, manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
    writeFileAt(
      worktreeDir,
      `${artifactRoot}/README.md`,
      [
        '# OpenSession PR Artifacts',
        '',
        `- Repo: \`${repoFullName}\``,
        `- PR: #${prNumber}`,
        `- Review ID: \`${reviewId}\``,
        `- Generated at (UTC): ${generatedAt}`,
        '',
        'This branch is generated by the Session Review workflow.',
        'It mirrors review JSONL/meta files for linkable inspection in GitHub UI.',
        '',
      ].join('\n'),
    );

    runGit(['add', '.'], { cwd: worktreeDir });
    const staged = runGit(['status', '--porcelain'], { cwd: worktreeDir });
    if (staged) {
      runGit(
        ['commit', '-m', `opensession review artifacts pr#${prNumber} ${shortSha(head)}`],
        { cwd: worktreeDir },
      );
    } else {
      runGit(['commit', '--allow-empty', '-m', `opensession review artifacts pr#${prNumber} ${shortSha(head)}`], {
        cwd: worktreeDir,
      });
    }
    runGit(['push', '--force', 'origin', `${branchName}:${branchName}`], { cwd: worktreeDir });
  } catch (error) {
    publishError = error instanceof Error ? error.message : String(error);
  } finally {
    runGit(['worktree', 'remove', '--force', worktreeDir], { allowFail: true });
    fs.rmSync(tmpBase, { recursive: true, force: true });
  }

  return {
    enabled: true,
    branchName,
    artifactRoot,
    manifestPath,
    error: publishError,
    treeLink,
  };
}

function renderReport({
  marker,
  mode,
  ledgerRef,
  repoFullName,
  prNumber,
  generatedAt,
  base,
  head,
  commits,
  sessions,
  changedFiles = [],
  testFiles = [],
  qaDigest = null,
  missingLedgerRef = false,
  artifact = null,
}) {
  const title = mode === 'final'
    ? 'OpenSession Review (final snapshot)'
    : 'OpenSession Review';
  const lines = [];
  const areaSummary = collectAreaSummary(changedFiles);
  const qas = qaDigest?.pairs ?? [];
  const reviewId = buildReviewId(repoFullName, prNumber, head);
  const prLinks = pullRequestLinks(repoFullName, prNumber, base, head);
  const sessionsPerCommit = commits.length > 0
    ? (sessions.length / commits.length).toFixed(1)
    : '0.0';

  lines.push(marker);
  lines.push(`### ${title}`);
  lines.push('');
  lines.push('| Metric | Value |');
  lines.push('| --- | --- |');
  lines.push(`| Ledger ref | \`${ledgerRef}\` |`);
  if (base && head) {
    lines.push(`| Base -> Head | \`${base}\` -> \`${head}\` |`);
  } else if (head) {
    lines.push(`| Head | \`${head}\` |`);
  }
  lines.push(`| Updated at (UTC) | ${generatedAt} |`);
  lines.push(`| Commit range size | ${commits.length} |`);
  lines.push(`| Primary linked sessions | ${sessions.length} |`);
  lines.push(`| Modified files | ${changedFiles.length} |`);
  lines.push(`| Added/updated test files | ${testFiles.length} |`);
  lines.push(`| Session scope | primary only (auxiliary filtered) |`);
  if (prLinks) {
    lines.push(`| Quick links | [Files changed](${prLinks.files}) · [Commits](${prLinks.commits})${prLinks.compare ? ` · [Compare](${prLinks.compare})` : ''} |`);
  }
  if (reviewId && prLinks) {
    lines.push(`| Local review | [Open in UI](${localReviewLink(reviewId)}) · \`ops review ${prLinks.files.replace('/files', '')}\` |`);
  }
  if (artifact?.enabled && artifact?.branchName && artifact?.treeLink) {
    const manifestLink = artifact?.manifestPath && artifact?.branchName
      ? githubBlobLink(repoFullName, artifact.branchName, artifact.manifestPath)
      : null;
    lines.push(`| Artifact branch | [\`${artifact.branchName}\`](${artifact.treeLink})${manifestLink ? ` · [manifest.json](${manifestLink})` : ''} |`);
  }
  if (artifact?.enabled && artifact?.error) {
    lines.push(`| Artifact publish | failed (\`${artifact.error}\`) |`);
  }
  lines.push(`| Ledger status | ${missingLedgerRef ? `missing (\`${ledgerRef}\`)` : 'available'} |`);
  lines.push('');

  lines.push('#### Reviewer Quick Digest');
  lines.push('| Q&A | Areas | Files | Tests | Sessions / Commit |');
  lines.push('| ---: | ---: | ---: | ---: | ---: |');
  lines.push(`| ${qas.length} | ${areaSummary.length} | ${changedFiles.length} | ${testFiles.length} | ${sessionsPerCommit} |`);
  lines.push('');

  if (areaSummary.length > 0) {
    lines.push('#### Area Summary');
    lines.push('| Area | Files changed |');
    lines.push('| --- | ---: |');
    for (const item of areaSummary.slice(0, 8)) {
      lines.push(`| ${escapeTableCell(item.area)} | ${item.count} |`);
    }
    lines.push('');
  }

  if (missingLedgerRef) {
    lines.push('No ledger ref found for this branch yet. Push at least one tracked session and retry.');
    return lines.join('\n');
  }

  if (qas.length > 0) {
    lines.push('#### Interactive Q&A');
    lines.push('| Session | Commit | Question | Answer |');
    lines.push('| --- | --- | --- | --- |');
    for (const pair of qas) {
      lines.push(
        `| \`${escapeTableCell(pair.session_id ?? '-')}\` | ${pair.commit ? commitLink(repoFullName, pair.commit) : '-'} | ${escapeTableCell(pair.question)} | ${escapeTableCell(pair.answer ?? '-')} |`,
      );
    }
    lines.push('');
  } else {
    lines.push('#### Interactive Q&A');
    lines.push('No interactive Q&A excerpts were captured from primary sessions.');
    lines.push('');
  }

  pushDetailsList(lines, `Changed paths (${changedFiles.length})`, changedFiles, (filePath) => `\`${filePath}\``);
  pushDetailsList(lines, `Added/updated tests (${testFiles.length})`, testFiles, (filePath) => `\`${filePath}\``);

  if (commits.length > 0) {
    pushDetailsList(
      lines,
      `Commit trail (${commits.length})`,
      commits,
      (sha) => commitLink(repoFullName, sha),
      20,
    );
  }

  if (sessions.length === 0) {
    lines.push('No indexed sessions matched this commit range.');
    return lines.join('\n');
  }

  lines.push('<details>');
  lines.push(`<summary>Linked sessions (${sessions.length})</summary>`);
  lines.push('');
  lines.push('| Session ID | Tool | Files | Commits | Open | OpenSession | JSONL | Meta | Title |');
  lines.push('| --- | --- | ---: | --- | --- | --- | --- | --- | --- |');
  for (const session of sessions.slice(0, 50)) {
    const commitCell = session.commits.length > 0
      ? session.commits
          .slice(0, 3)
          .map((sha) => commitLink(repoFullName, sha))
          .join(', ')
      : String(session.commits.length);
    const suffix = session.commits.length > 3 ? ` +${session.commits.length - 3}` : '';
    const primaryCommit = session.commits[0] ?? '';
    const openLink = localReviewLink(reviewId, session.session_id, primaryCommit);
    const webLink =
      artifact?.enabled && artifact?.branchName && artifact?.artifactRoot && session.hail_path
        ? opensessionSourceLink(
            repoFullName,
            artifact.branchName,
            `${artifact.artifactRoot}/${session.hail_path}`,
          )
        : null;
    const hailLink =
      artifact?.enabled && artifact?.branchName && artifact?.artifactRoot && session.hail_path
        ? githubBlobLink(
            repoFullName,
            artifact.branchName,
            `${artifact.artifactRoot}/${session.hail_path}`,
          )
        : null;
    const metaLink =
      artifact?.enabled && artifact?.branchName && artifact?.artifactRoot && session.meta_path
        ? githubBlobLink(
            repoFullName,
            artifact.branchName,
            `${artifact.artifactRoot}/${session.meta_path}`,
          )
        : null;
    lines.push(
      `| \`${session.session_id}\` | ${escapeTableCell(session.tool || '-')} | ${session.files_changed ?? 0} | ${commitCell}${suffix} | ${openLink ? `[open](${openLink})` : '-'} | ${webLink ? `[web](${webLink})` : '-'} | ${hailLink ? `[jsonl](${hailLink})` : '-'} | ${metaLink ? `[meta](${metaLink})` : `\`${session.meta_path ?? ''}\``} | ${escapeTableCell(session.title || '-')} |`,
    );
  }
  if (sessions.length > 50) {
    lines.push(`| ... | ... | ... | ... | ... | ... | ... | ... | ...and ${sessions.length - 50} more sessions. |`);
  }
  lines.push('</details>');
  return lines.join('\n');
}

function main() {
  const args = parseArgs(process.argv);
  const mode = args.mode ?? 'update';
  const ledgerRef = args['ledger-ref'];
  const repoFullName = args.repo ?? '';
  const prNumberRaw = args['pr-number'] ?? '';
  const prNumber = /^\d+$/.test(prNumberRaw) ? Number(prNumberRaw) : null;
  const publishArtifacts = (args['publish-artifacts'] ?? 'false') === 'true';
  const base = args.base ?? '';
  const head = args.head ?? '';
  const generatedAt = new Date().toISOString();
  const marker = mode === 'final'
    ? '<!-- opensession-session-review-final -->'
    : '<!-- opensession-session-review -->';

  if (!ledgerRef) {
    console.log(`${marker}\n### OpenSession Review\n\nLedger ref is missing.`);
    return;
  }

  const refExists = gitCommandSucceeds(`git show-ref --verify --quiet ${ledgerRef}`);
  const commits = collectCommitRange(base, head);
  const sessions = refExists ? collectIndexedSessions(ledgerRef, commits) : [];
  const changedFiles = collectChangedFiles(base, head);
  const testFiles = changedFiles.filter((filePath) => isTestFilePath(filePath));
  const qaDigest = refExists
    ? collectQaDigestFromSessions(ledgerRef, sessions)
    : { pairs: [] };
  const reviewId = buildReviewId(repoFullName, prNumber, head);
  const artifact = publishArtifactsBranch({
    enabled: publishArtifacts,
    repoFullName,
    prNumber,
    head,
    base,
    ledgerRef,
    reviewId,
    generatedAt,
    commits,
    sessions,
  });
  const report = renderReport({
    marker,
    mode,
    ledgerRef,
    repoFullName,
    prNumber,
    generatedAt,
    base,
    head,
    commits,
    sessions,
    changedFiles,
    testFiles,
    qaDigest,
    missingLedgerRef: !refExists,
    artifact,
  });
  console.log(report);
}

const isMainModule = process.argv[1]
  ? import.meta.url === pathToFileURL(process.argv[1]).href
  : false;

if (isMainModule) {
  main();
}

export {
  buildArtifactBranchName,
  buildArtifactRoot,
  buildReviewId,
  collectAreaSummary,
  collectChangedFiles,
  collectCommitRange,
  collectIndexedSessions,
  collectQaDigestFromSessions,
  isTestFilePath,
  localReviewLink,
  normalizeDigestText,
  renderReport,
};
