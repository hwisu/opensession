#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

function parseArgs(argv) {
	const args = {};
	for (let i = 0; i < argv.length; i += 1) {
		const token = argv[i];
		if (!token.startsWith('--')) continue;
		const key = token.slice(2);
		const value = argv[i + 1];
		if (!value || value.startsWith('--')) {
			args[key] = true;
			continue;
		}
		args[key] = value;
		i += 1;
	}
	return args;
}

function collectMetricFiles(rootDir) {
	const out = [];
	const stack = [rootDir];
	while (stack.length > 0) {
		const current = stack.pop();
		const entries = fs.readdirSync(current, { withFileTypes: true });
		for (const entry of entries) {
			const fullPath = path.join(current, entry.name);
			if (entry.isDirectory()) {
				stack.push(fullPath);
				continue;
			}
			if (entry.isFile() && entry.name === 'metrics.json') {
				out.push(fullPath);
			}
		}
	}
	return out;
}

function percentile95(values) {
	if (values.length === 0) return 0;
	const sorted = [...values].sort((a, b) => a - b);
	const index = Math.max(0, Math.ceil(sorted.length * 0.95) - 1);
	return sorted[index];
}

function appendSummary(markdown) {
	const summaryFile = process.env.GITHUB_STEP_SUMMARY;
	if (!summaryFile) return;
	fs.appendFileSync(summaryFile, `${markdown}\n`, 'utf8');
}

function main() {
	const args = parseArgs(process.argv.slice(2));
	const inputDir = path.resolve(String(args['input-dir'] ?? '.ci-diagnostics'));
	const outputPath = path.resolve(
		String(args.output ?? path.join(inputDir, 'desktop-metrics-summary.json')),
	);

	if (!fs.existsSync(inputDir)) {
		console.error(`[desktop-build-metrics] input dir does not exist: ${inputDir}`);
		process.exit(1);
	}

	const metricFiles = collectMetricFiles(inputDir);
	if (metricFiles.length === 0) {
		console.error(`[desktop-build-metrics] no metrics.json files found under ${inputDir}`);
		process.exit(1);
	}

	const rows = metricFiles
		.map((filePath) => ({
			filePath,
			...JSON.parse(fs.readFileSync(filePath, 'utf8')),
		}))
		.filter((row) => Number.isFinite(row.build_seconds));

	if (rows.length === 0) {
		console.error('[desktop-build-metrics] no valid metric samples with numeric build_seconds');
		process.exit(1);
	}

	const buildSeconds = rows.map((row) => Number(row.build_seconds));
	const appBytes = rows.map((row) => Number(row.app_bytes ?? 0));
	const dmgBytes = rows.map((row) => Number(row.dmg_bytes ?? 0));

	const summary = {
		samples: rows.length,
		build_seconds: {
			min: Math.min(...buildSeconds),
			max: Math.max(...buildSeconds),
			p95: percentile95(buildSeconds),
		},
		app_bytes: {
			max: Math.max(...appBytes),
		},
		dmg_bytes: {
			max: Math.max(...dmgBytes),
		},
	};

	fs.mkdirSync(path.dirname(outputPath), { recursive: true });
	fs.writeFileSync(outputPath, `${JSON.stringify(summary, null, 2)}\n`, 'utf8');

	const markdown = [
		'### Desktop Build Metrics',
		`- samples: ${summary.samples}`,
		`- build_seconds min/max/p95: ${summary.build_seconds.min}/${summary.build_seconds.max}/${summary.build_seconds.p95}`,
		`- app_bytes max: ${summary.app_bytes.max}`,
		`- dmg_bytes max: ${summary.dmg_bytes.max}`,
		`- summary_json: \`${path.relative(process.cwd(), outputPath)}\``,
	].join('\n');

	console.log(markdown);
	appendSummary(markdown);
}

main();
