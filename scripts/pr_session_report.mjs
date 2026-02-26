#!/usr/bin/env node

import { execSync } from 'node:child_process';

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

function localReviewLink(reviewId, sessionId = '', commitSha = '') {
  if (!reviewId) return null;
  const url = new URL(`http://127.0.0.1:8788/review/local/${reviewId}`);
  if (sessionId) url.searchParams.set('session', sessionId);
  if (commitSha) url.searchParams.set('commit', commitSha);
  return url.toString();
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

  lines.push('| Session ID | Commits | Open | Meta |');
  lines.push('| --- | ---: | --- | --- |');
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
    lines.push(
      `| \`${session.session_id}\` | ${commitCell}${suffix} | ${openLink ? `[open](${openLink})` : '-'} | \`${session.meta_path ?? ''}\` |`,
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
  });
  console.log(report);
}

main();
