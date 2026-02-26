#!/usr/bin/env node

import { execFileSync, execSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

function run(cmd) {
  return execSync(cmd, { stdio: ['ignore', 'pipe', 'pipe'], encoding: 'utf8' }).trim();
}

function tryRun(cmd) {
  try {
    return run(cmd);
  } catch {
    return '';
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
  const { cwd = process.cwd(), allowFail = false } = options;
  try {
    return execFileSync('git', args, {
      cwd,
      stdio: ['ignore', 'pipe', 'pipe'],
      encoding: 'utf8',
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

function collectIndexedSessions(ledgerRef, commits) {
  const bySession = new Map();
  for (const sha of commits) {
    const prefix = `v1/index/commits/${sha}`;
    const listing = tryRun(`git ls-tree -r --name-only ${ledgerRef} -- ${prefix}`);
    if (!listing) continue;

    for (const relPath of listing.split('\n').map((line) => line.trim()).filter(Boolean)) {
      const raw = tryRun(`git show ${ledgerRef}:${relPath}`);
      if (!raw) continue;
      let payload = null;
      try {
        payload = JSON.parse(raw);
      } catch {
        continue;
      }
      const sessionId = payload.session_id;
      if (!sessionId) continue;
      const existing = bySession.get(sessionId) ?? {
        session_id: sessionId,
        commits: [],
        hail_path: payload.hail_path,
        meta_path: payload.meta_path,
      };
      existing.commits = unique([...existing.commits, sha]);
      bySession.set(sessionId, existing);
    }
  }
  return Array.from(bySession.values()).sort((a, b) => a.session_id.localeCompare(b.session_id));
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
        const metaBody = tryRun(`git show ${ledgerRef}:${session.meta_path}`);
        if (metaBody) writeFileAt(worktreeDir, metaArtifactPath, ensureTrailingNewline(metaBody));
      }
      if (session.hail_path) {
        const hailBody = tryRun(`git show ${ledgerRef}:${session.hail_path}`);
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
  missingLedgerRef = false,
  artifact = null,
}) {
  const title = mode === 'final'
    ? 'OpenSession Review (final snapshot)'
    : 'OpenSession Review';
  const lines = [];
  lines.push(marker);
  lines.push(`### ${title}`);
  lines.push('');
  lines.push(`- Ledger ref: \`${ledgerRef}\``);
  if (base) lines.push(`- Base: \`${base}\``);
  if (head) lines.push(`- Head: \`${head}\``);
  lines.push(`- Updated at (UTC): ${generatedAt}`);
  lines.push(`- Commit range size: ${commits.length}`);
  lines.push(`- Linked sessions: ${sessions.length}`);
  const prLinks = pullRequestLinks(repoFullName, prNumber, base, head);
  const reviewId = buildReviewId(repoFullName, prNumber, head);
  if (prLinks) {
    lines.push(
      `- Quick links: [Files changed](${prLinks.files}) · [Commits](${prLinks.commits})${prLinks.compare ? ` · [Compare](${prLinks.compare})` : ''}`,
    );
  }
  if (reviewId && prLinks) {
    lines.push(`- Local review: [Open in UI](${localReviewLink(reviewId)})`);
    lines.push(`- CLI: \`ops review ${prLinks.files.replace('/files', '')}\``);
  }
  if (artifact?.enabled && artifact?.branchName && artifact?.treeLink) {
    lines.push(`- Artifact branch: [\`${artifact.branchName}\`](${artifact.treeLink})`);
  }
  if (artifact?.enabled && artifact?.manifestPath && artifact?.branchName) {
    const manifestLink = githubBlobLink(
      repoFullName,
      artifact.branchName,
      artifact.manifestPath,
    );
    if (manifestLink) {
      lines.push(`- Artifact manifest: [manifest.json](${manifestLink})`);
    }
  }
  if (artifact?.enabled && artifact?.error) {
    lines.push(`- Artifact publish: failed (\`${artifact.error}\`)`);
  }
  if (missingLedgerRef) {
    lines.push(`- Ledger status: missing (\`${ledgerRef}\`)`);
  }
  lines.push('');

  if (commits.length > 0) {
    lines.push('#### Commit trail');
    for (const sha of commits.slice(0, 20)) {
      lines.push(`- ${commitLink(repoFullName, sha)}`);
    }
    if (commits.length > 20) {
      lines.push(`- ...and ${commits.length - 20} more`);
    }
    lines.push('');
  }

  if (missingLedgerRef) {
    lines.push('No ledger ref found for this branch yet. Push at least one tracked session and retry.');
    return lines.join('\n');
  }

  if (sessions.length === 0) {
    lines.push('No indexed sessions matched this commit range.');
    return lines.join('\n');
  }

  lines.push('| Session ID | Commits | Open | JSONL | Meta |');
  lines.push('| --- | ---: | --- | --- | --- |');
  for (const session of sessions.slice(0, 50)) {
    const commitCell = session.commits.length > 0
      ? session.commits
          .slice(0, 4)
          .map((sha) => commitLink(repoFullName, sha))
          .join(', ')
      : String(session.commits.length);
    const suffix = session.commits.length > 4 ? ` +${session.commits.length - 4}` : '';
    const primaryCommit = session.commits[0] ?? '';
    const openLink = localReviewLink(reviewId, session.session_id, primaryCommit);
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
      `| \`${session.session_id}\` | ${commitCell}${suffix} | ${openLink ? `[open](${openLink})` : '-'} | ${hailLink ? `[jsonl](${hailLink})` : '-'} | ${metaLink ? `[meta](${metaLink})` : `\`${session.meta_path ?? ''}\``} |`,
    );
  }
  if (sessions.length > 50) {
    lines.push('');
    lines.push(`...and ${sessions.length - 50} more sessions.`);
  }
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

  const refExists = tryRun(`git show-ref ${ledgerRef}`);
  const commits = collectCommitRange(base, head);
  const sessions = refExists ? collectIndexedSessions(ledgerRef, commits) : [];
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
    missingLedgerRef: !refExists,
    artifact,
  });
  console.log(report);
}

main();
