#!/usr/bin/env node

import { spawnSync } from 'node:child_process';

function parseArgs(argv) {
	const parsed = {};
	for (let i = 0; i < argv.length; i += 1) {
		const token = argv[i];
		if (!token.startsWith('--')) continue;
		const key = token.slice(2);
		const next = argv[i + 1];
		if (!next || next.startsWith('--')) {
			parsed[key] = true;
			continue;
		}
		parsed[key] = next;
		i += 1;
	}
	return parsed;
}

function platformToOs(platform) {
	if (platform === 'darwin') return 'macos';
	if (platform === 'linux') return 'linux';
	return platform;
}

function run(command, args, options = {}) {
	const result = spawnSync(command, args, {
		stdio: options.capture ? 'pipe' : 'inherit',
		encoding: options.capture ? 'utf8' : undefined,
		env: { ...process.env, ...(options.env ?? {}) },
	});
	return result;
}

function ensureCommand(command, errors, remediation) {
	const result = run(command, ['--version'], { capture: true });
	if (result.error || result.status !== 0) {
		errors.push(`${command}: command not found`);
		if (remediation) errors.push(remediation);
		return false;
	}
	return true;
}

function ensureMiseTools(errors) {
	if (!ensureCommand('mise', errors, 'Install mise and run `mise install` in repo root.')) {
		return;
	}
	const toolChecks = [
		['node', '--version'],
		['npm', '--version'],
		['cargo', '--version'],
		['rustc', '--version'],
	];
	for (const [tool, arg] of toolChecks) {
		const result = run('mise', ['exec', '--', tool, arg], { capture: true });
		if (result.error || result.status !== 0) {
			errors.push(`${tool}: unavailable in mise toolchain`);
		}
	}
	if (errors.some((line) => line.includes('unavailable in mise toolchain'))) {
		errors.push('Run `mise install` to provision node/rust toolchain from mise.toml.');
	}
}

function ensureUniversalTargets(errors) {
	if (!ensureCommand('rustup', errors, 'Install rustup and run `rustup target add aarch64-apple-darwin x86_64-apple-darwin`.')) {
		return;
	}
	const installed = run('rustup', ['target', 'list', '--installed'], { capture: true });
	if (installed.error || installed.status !== 0) {
		errors.push('failed to read installed Rust targets via rustup');
		return;
	}
	const targets = new Set(
		(installed.stdout ?? '')
			.split(/\r?\n/)
			.map((line) => line.trim())
			.filter(Boolean),
	);
	for (const target of ['aarch64-apple-darwin', 'x86_64-apple-darwin']) {
		if (!targets.has(target)) {
			errors.push(`missing Rust target: ${target}`);
		}
	}
	if (errors.some((line) => line.startsWith('missing Rust target'))) {
		errors.push('Run `rustup target add aarch64-apple-darwin x86_64-apple-darwin`.');
	}
}

function ensureLinuxDesktopDeps(errors) {
	const requiredCommands = ['pkg-config', 'xvfb-run', 'patchelf'];
	for (const command of requiredCommands) {
		ensureCommand(
			command,
			errors,
			'Install desktop build deps: `sudo apt-get install -y pkg-config xvfb patchelf libgtk-3-dev libwebkit2gtk-4.1-dev`.',
		);
	}
	const requiredPkgConfig = ['gtk+-3.0', 'webkit2gtk-4.1'];
	for (const pkg of requiredPkgConfig) {
		const result = run('pkg-config', ['--exists', pkg], { capture: true });
		if (result.error || result.status !== 0) {
			errors.push(`pkg-config package missing: ${pkg}`);
		}
	}
	if (errors.some((line) => line.startsWith('pkg-config package missing:'))) {
		errors.push(
			'Install Linux GUI libs: `sudo apt-get install -y libgtk-3-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev`.',
		);
	}
}

function ensureAppleSecrets(errors) {
	const required = [
		'APPLE_CERTIFICATE',
		'APPLE_CERTIFICATE_PASSWORD',
		'APPLE_SIGNING_IDENTITY',
		'APPLE_ID',
		'APPLE_PASSWORD',
		'APPLE_TEAM_ID',
	];
	for (const key of required) {
		if (!process.env[key]) errors.push(`missing required env: ${key}`);
	}
}

function main() {
	const args = parseArgs(process.argv.slice(2));
	const mode = String(args.mode ?? 'local');
	const os = String(args.os ?? platformToOs(process.platform));

	const requireMise = args['require-mise'] === true || mode === 'local';
	const requireUniversalTargets =
		args['check-universal-targets'] === true || os === 'macos';
	const requireLinuxDeps = args['check-linux-deps'] === true || os === 'linux';
	const requireAppleSecrets =
		args['check-apple-secrets'] === true || mode === 'release';

	const errors = [];
	if (requireMise) ensureMiseTools(errors);
	if (requireUniversalTargets) ensureUniversalTargets(errors);
	if (requireLinuxDeps) ensureLinuxDesktopDeps(errors);
	if (requireAppleSecrets) ensureAppleSecrets(errors);

	if (errors.length > 0) {
		console.error('[desktop-build-preflight] FAILED');
		for (const entry of errors) {
			console.error(` - ${entry}`);
		}
		process.exit(1);
	}

	console.log(`[desktop-build-preflight] OK (mode=${mode}, os=${os})`);
}

main();
