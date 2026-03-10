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

  assert.match(report, /\| Metric \| Value \|/);
  assert.match(report, /\| Q&A \| Areas \| Files \| Tests \| Sessions \/ Commit \|/);
  assert.match(report, /#### Area Summary/);
  assert.match(report, /\| Session \| Commit \| Question \| Answer \|/);
  assert.match(report, /<details>\n<summary>Changed paths \(4\)<\/summary>/);
  assert.match(report, /<details>\n<summary>Linked sessions \(1\)<\/summary>/);
  assert.match(
    report,
    /\| Session ID \| Tool \| Files \| Commits \| Open \| OpenSession \| JSONL \| Meta \| Title \|/,
  );
  assert.match(report, /primary only \(auxiliary filtered\)/);
});
