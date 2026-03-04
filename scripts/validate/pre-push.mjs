#!/usr/bin/env node

import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const repoRoot = process.cwd();
const cacheDir = path.join(repoRoot, '.opensession', '.cache', 'pre-push');

function resolveRustCargoCommand() {
	const rustup = spawnSync('rustup', ['which', '--toolchain', 'stable', 'cargo'], {
		stdio: 'pipe',
		encoding: 'utf8',
	});
	const rustc = spawnSync('rustup', ['which', '--toolchain', 'stable', 'rustc'], {
		stdio: 'pipe',
		encoding: 'utf8',
	});
	if (rustup.status === 0 && rustc.status === 0) {
		const cargoBin = rustup.stdout.trim();
		const rustcBin = rustc.stdout.trim();
		if (cargoBin && rustcBin) {
			const toolchainBin = path.dirname(rustcBin);
			return {
				cargoCmd: cargoBin,
				env: {
					PATH: `${toolchainBin}${path.delimiter}${process.env.PATH ?? ''}`,
					RUSTC: rustcBin,
				},
				usingRustupStable: true,
			};
		}
	}
	return {
		cargoCmd: 'cargo',
		env: {},
		usingRustupStable: false,
	};
}

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

function hasCommand(command) {
	const result = spawnSync(command, ['--version'], {
		stdio: 'ignore',
	});
	if (result.error) return false;
	return result.status === 0;
}

function runCargo(rustToolchain, args) {
	run(rustToolchain.cargoCmd, args, { env: rustToolchain.env });
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
		run('npm', ['ci', '--prefer-offline', '--no-audit', '--no-fund', '--silent'], {
			cwd: pkgPath,
		});
		fs.mkdirSync(cacheDir, { recursive: true });
		fs.writeFileSync(cachePath, `${lockHash}\n`, 'utf8');
	}
}

function main() {
	console.log('Running pre-push validation pipeline (pre-commit -> pre-push)...');
	const rustToolchain = resolveRustCargoCommand();
	run('sh', ['.githooks/pre-commit']);

	if (!hasCommand('npm')) {
		console.error('ERROR: npm is required for pre-push validation.');
		console.error('Install Node.js/npm and re-run ./.githooks/pre-push.');
		process.exit(1);
	}

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
		run('npm', ['run', 'test', '--silent'], { cwd: path.join(repoRoot, 'packages/ui') });
		run('npm', ['run', 'check'], { cwd: path.join(repoRoot, 'web') });
	}

	runCargo(rustToolchain, ['clippy', '--workspace', '--quiet', '--', '-D', 'warnings']);

	if (rustToolchain.usingRustupStable) {
		run('rustup', ['target', 'add', 'wasm32-unknown-unknown', '--toolchain', 'stable']);
	}
	runCargo(rustToolchain, [
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
	runCargo(rustToolchain, ['test', '--workspace', '--exclude', 'opensession-e2e', '--quiet']);
	runCargo(rustToolchain, ['test', '-p', 'opensession-e2e', '--no-run', '--quiet']);

	console.log('pre-push validation pipeline passed.');
}

main();
