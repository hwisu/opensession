#!/usr/bin/env node

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

const args = parseArgs(process.argv);
const branch = args.branch || process.env.BRANCH || 'detached';
const encoded = Buffer.from(branch, 'utf8').toString('base64url');
process.stdout.write(`refs/opensession/branches/${encoded}`);
