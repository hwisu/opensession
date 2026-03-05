#!/usr/bin/env node

import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const repoRoot = process.cwd();
const cacheDir = path.join(repoRoot, '.opensession', '.cache', 'pre-push');

function run(cmd, args, opts = {}) {
	const result = spawnSync(cmd, args, {
		cwd: opts.cwd ?? repoRoot,
		stdio: 'inherit',
		env: { ...process.env, ...(opts.env ?? {}) },
	});
	if (result.error) {
		throw result.error;
	}
	if (result.status !== 0) {
		process.exit(result.status ?? 1);
	}
}

function assertMiseAvailable() {
	const result = spawnSync('mise', ['--version'], {
		stdio: 'ignore',
	});
	if (result.error || result.status !== 0) {
		console.error('ERROR: mise is required for pre-push validation.');
		console.error('Install mise, then run: mise install');
		console.error('https://mise.jdx.dev/getting-started.html');
		process.exit(1);
	}
}

function runMise(command, args, opts = {}) {
	run('mise', ['exec', '--', command, ...args], opts);
}

function sha256File(filePath) {
	const hash = crypto.createHash('sha256');
	hash.update(fs.readFileSync(filePath));
	return hash.digest('hex');
}

function maybeNpmCi({ packageDir, lockFile, cacheKeyName }) {
	const lockPath = path.join(repoRoot, lockFile);
	const pkgPath = path.join(repoRoot, packageDir);
	const cachePath = path.join(cacheDir, cacheKeyName);
	const lockHash = sha256File(lockPath);
	const nodeModulesPath = path.join(pkgPath, 'node_modules');

	let shouldInstall = true;
	if (fs.existsSync(cachePath) && fs.existsSync(nodeModulesPath)) {
		const cached = fs.readFileSync(cachePath, 'utf8').trim();
		if (cached === lockHash) {
			console.log(`Skipping npm ci for ${packageDir} (cache hit at .opensession/.cache/pre-push)`);
			shouldInstall = false;
		}
	}

	if (shouldInstall) {
		console.log(`Running npm ci for ${packageDir}...`);
		runMise('npm', ['ci', '--prefer-offline', '--no-audit', '--no-fund', '--silent'], {
			cwd: pkgPath,
		});
		fs.mkdirSync(cacheDir, { recursive: true });
		fs.writeFileSync(cachePath, `${lockHash}\n`, 'utf8');
	}
}

function main() {
	console.log('Running pre-push validation pipeline (pre-commit -> pre-push)...');
	assertMiseAvailable();
	run('sh', ['.githooks/pre-commit']);
	runMise('node', ['scripts/validate/desktop-build-preflight.mjs', '--mode', 'local']);

	if (process.env.OPENSESSION_SKIP_FRONTEND_PRE_PUSH === '1') {
		console.log('No frontend-related changes vs upstream; skipping frontend checks');
	} else {
		maybeNpmCi({
			packageDir: 'packages/ui',
			lockFile: 'packages/ui/package-lock.json',
			cacheKeyName: 'packages-ui.lock.sha256',
		});
		maybeNpmCi({
			packageDir: 'web',
			lockFile: 'web/package-lock.json',
			cacheKeyName: 'web.lock.sha256',
		});
		runMise('npm', ['run', 'test', '--silent'], { cwd: path.join(repoRoot, 'packages/ui') });
		runMise('npm', ['run', 'check'], { cwd: path.join(repoRoot, 'web') });
	}

	runMise('cargo', ['clippy', '--workspace', '--quiet', '--', '-D', 'warnings']);
	runMise('rustup', ['target', 'add', 'wasm32-unknown-unknown']);
	runMise('cargo', [
		'clippy',
		'--manifest-path',
		'crates/worker/Cargo.toml',
		'--target',
		'wasm32-unknown-unknown',
		'--quiet',
		'--',
		'-D',
		'warnings',
	]);
	runMise('cargo', ['test', '--workspace', '--exclude', 'opensession-e2e', '--quiet']);
	runMise('cargo', ['test', '-p', 'opensession-e2e', '--no-run', '--quiet']);

	console.log('pre-push validation pipeline passed.');
}

main();
