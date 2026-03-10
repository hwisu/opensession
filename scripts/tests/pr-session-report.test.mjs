import assert from 'node:assert/strict';
import test from 'node:test';

import { buildArtifactBranchName, renderReport } from '../pr_session_report.mjs';

test('buildArtifactBranchName accepts persistent branch override', () => {
  assert.equal(buildArtifactBranchName(18), 'opensession/pr-18-sessions');
  assert.equal(buildArtifactBranchName(18, 'pr/sessions'), 'pr/sessions');
});

test('renderReport emphasizes metrics, areas, and linked session metadata', () => {
  const report = renderReport({
    marker: '<!-- opensession-session-review -->',
    mode: 'update',
    ledgerRef: 'refs/opensession/branches/example',
    repoFullName: 'acme/opensession',
    prNumber: 18,
    generatedAt: '2026-03-09T09:44:22.436Z',
    base: 'bf7d505805dd45cc54dd2e064757fbab2dec2bd2',
    head: '5297b09fbf250feb50bce4986f4e2148e41eb347',
    commits: [
      '5297b09fbf250feb50bce4986f4e2148e41eb347',
      '688f6192f0f250feb50bce4986f4e2148e41eb34',
    ],
    sessions: [
      {
        session_id: 's-primary-1',
        tool: 'codex',
        title: 'Refactor review pipeline',
        files_changed: 7,
        commits: ['5297b09fbf250feb50bce4986f4e2148e41eb347'],
        hail_path: 'v1/aa/s-primary-1.hail.jsonl',
        meta_path: 'v1/aa/s-primary-1.meta.json',
      },
    ],
    changedFiles: [
      'crates/cli/src/review.rs',
      'scripts/pr_session_report.mjs',
      '.github/workflows/session-review.yml',
      'README.md',
    ],
    testFiles: ['crates/cli/tests/handoff_cli/review_cli.rs'],
    qaDigest: {
      pairs: [
        {
          session_id: 's-primary-1',
          commit: '5297b09fbf250feb50bce4986f4e2148e41eb347',
          question: 'scope?',
          answer: 'filter auxiliary sessions',
        },
      ],
    },
    missingLedgerRef: false,
    artifact: {
      enabled: true,
      branchName: 'opensession/pr-18-sessions',
      artifactRoot: 'reviews/gh-acme-opensession-pr18-5297b09',
      manifestPath: 'reviews/gh-acme-opensession-pr18-5297b09/manifest.json',
      treeLink: 'https://github.com/acme/opensession/tree/opensession/pr-18-sessions',
      error: null,
    },
  });

  assert.match(report, /Snapshot for `acme\/opensession` PR #18\./);
  assert.doesNotMatch(report, /This run covers 2 commits/);
  assert.match(report, /- \*\*Comment type:\*\* sticky update/);
  assert.match(report, /- \*\*Review ID:\*\* `gh-acme-opensession-pr18-5297b09`/);
  assert.match(report, /- \*\*Coverage:\*\* 1 Q&A excerpt, 4 changed areas, 4 modified files, 1 added or updated test file, 0\.5 sessions per commit\./);
  assert.match(report, /- \*\*Ledger:\*\* available on `refs\/opensession\/branches\/example`/);
  assert.match(report, /- \*\*Top areas:\*\* `\.github` \(1\), `\(root\)` \(1\), `crates` \(1\), `scripts` \(1\)\./);
  assert.match(report, /- \*\*Local replay:\*\* `opensession review https:\/\/github\.com\/acme\/opensession\/pull\/18`/);
  assert.match(report, /- \*\*PR URL:\*\* https:\/\/github\.com\/acme\/opensession\/pull\/18/);
  assert.match(report, /- \*\*Artifact storage:\*\* ephemeral branch \(deleted on PR close\) on \[`opensession\/pr-18-sessions`]/);
  assert.match(report, /- \*\*Session:\*\* `s-primary-1` on \[`5297b09`]/);
  assert.match(report, /  \*\*Question:\*\* scope\?/);
  assert.match(report, /  \*\*Answer:\*\* filter auxiliary sessions/);
  assert.match(report, /<details>\n<summary>Changed paths \(4\)<\/summary>/);
  assert.match(report, /<details>\n<summary>Linked sessions \(1\)<\/summary>/);
  assert.match(report, /Artifact links: \[web]\(/);
  assert.doesNotMatch(report, /127\.0\.0\.1:8788/);
  assert.match(report, /primary only \(auxiliary filtered\)/);
  assert.doesNotMatch(report, /#### Area Summary/);
  assert.equal((report.match(/\*\*Ledger:/g) ?? []).length, 1);
  assert.doesNotMatch(report, /\| Metric \| Value \|/);
  assert.doesNotMatch(report, /\| Session \| Commit \| Question \| Answer \|/);
});

test('renderReport does not claim unpublished ephemeral artifacts are live links in final snapshots', () => {
  const report = renderReport({
    marker: '<!-- opensession-session-review-final -->',
    mode: 'final',
    ledgerRef: 'refs/opensession/branches/example',
    repoFullName: 'acme/opensession',
    prNumber: 18,
    generatedAt: '2026-03-10T00:00:00.000Z',
    base: 'bf7d505805dd45cc54dd2e064757fbab2dec2bd2',
    head: '5297b09fbf250feb50bce4986f4e2148e41eb347',
    commits: ['5297b09fbf250feb50bce4986f4e2148e41eb347'],
    sessions: [],
    changedFiles: ['README.md'],
    testFiles: [],
    qaDigest: { pairs: [] },
    missingLedgerRef: false,
    artifact: {
      enabled: false,
      branchName: 'opensession/pr-18-sessions',
      artifactRoot: 'reviews/gh-acme-opensession-pr18-5297b09',
      manifestPath: 'reviews/gh-acme-opensession-pr18-5297b09/manifest.json',
      treeLink: 'https://github.com/acme/opensession/tree/opensession/pr-18-sessions',
      error: null,
      persistent: false,
    },
  });

  assert.match(
    report,
    /- \*\*Artifact storage:\*\* not published in this run; ephemeral cleanup policy applies on `opensession\/pr-18-sessions`, root `reviews\/gh-acme-opensession-pr18-5297b09`/,
  );
  assert.doesNotMatch(report, /\[manifest\.json]\(/);
  assert.doesNotMatch(report, /\[`opensession\/pr-18-sessions`]\(/);
});
