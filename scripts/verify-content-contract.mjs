#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';

const args = new Set(process.argv.slice(2));
const checkMode = args.has('--check');
const repoRoot = process.cwd();

function fail(message) {
	console.error(`[content-contract] ${message}`);
}

function toPosix(p) {
	return p.split(path.sep).join('/');
}

function readText(relPath) {
	const absPath = path.join(repoRoot, relPath);
	return fs.readFileSync(absPath, 'utf-8');
}

function readJson(relPath) {
	return JSON.parse(readText(relPath));
}

function ensureDirForFile(relPath) {
	const absPath = path.join(repoRoot, relPath);
	fs.mkdirSync(path.dirname(absPath), { recursive: true });
}

function unique(values) {
	return Array.from(new Set(values));
}

function parseDocsOutline(markdown) {
	const lines = markdown.split(/\r?\n/);
	const chapters = [];
	let current = null;
	let inFence = false;

	for (const line of lines) {
		const chapterMatch = line.match(/^##\s+(.+?)\s*$/);
		if (chapterMatch) {
			if (current) chapters.push(current);
			current = {
				heading: chapterMatch[1].trim(),
				subheadings: [],
				code_blocks: 0,
			};
			inFence = false;
			continue;
		}

		if (!current) continue;

		const subMatch = line.match(/^###\s+(.+?)\s*$/);
		if (subMatch) {
			current.subheadings.push(subMatch[1].trim());
			continue;
		}

		if (line.trimStart().startsWith('```')) {
			if (!inFence) {
				current.code_blocks += 1;
			}
			inFence = !inFence;
		}
	}

	if (current) chapters.push(current);
	return chapters;
}

function parseLandingOutline(source) {
	const sectionMatches = Array.from(source.matchAll(/data-contract-section="([^"]+)"/g)).map(
		(match) => match[1],
	);
	const capabilityMatches = Array.from(source.matchAll(/data-capability-key="([^"]+)"/g)).map(
		(match) => match[1],
	);

	return {
		sections: unique(sectionMatches).sort(),
		capabilities: unique(capabilityMatches).sort(),
	};
}

function stringifySnapshot(value) {
	return `${JSON.stringify(value, null, 2)}\n`;
}

function verifySnapshot(relPath, nextSnapshotText, issues) {
	const absPath = path.join(repoRoot, relPath);
	if (!fs.existsSync(absPath)) {
		issues.push(`Missing snapshot file: ${relPath}`);
		return;
	}

	const current = fs.readFileSync(absPath, 'utf-8');
	if (current !== nextSnapshotText) {
		issues.push(`Snapshot mismatch: ${relPath} (run: node scripts/verify-content-contract.mjs)`);
	}
}

function writeSnapshot(relPath, snapshotText) {
	ensureDirForFile(relPath);
	fs.writeFileSync(path.join(repoRoot, relPath), snapshotText, 'utf-8');
}

function main() {
	const contractPath = 'docs/content-contract.json';
	const contract = readJson(contractPath);
	const issues = [];

	const docsText = readText(contract.docs.path);
	const docsChapters = parseDocsOutline(docsText);
	const docsChapterMap = new Map(docsChapters.map((chapter) => [chapter.heading, chapter]));

	for (const heading of contract.docs.required_chapters) {
		const chapter = docsChapterMap.get(heading);
		if (!chapter) {
			issues.push(`Missing docs chapter: "${heading}"`);
			continue;
		}

		for (const subheading of contract.docs.required_template_subheadings) {
			if (!chapter.subheadings.includes(subheading)) {
				issues.push(`Missing template subheading "${subheading}" in chapter "${heading}"`);
			}
		}

		if (chapter.code_blocks < contract.docs.minimum_code_blocks_per_chapter) {
			issues.push(
				`Chapter "${heading}" has ${chapter.code_blocks} code block(s), minimum is ${contract.docs.minimum_code_blocks_per_chapter}`,
			);
		}
	}

	const landingText = readText(contract.landing.path);
	const landingOutline = parseLandingOutline(landingText);

	for (const section of contract.landing.required_sections) {
		if (!landingOutline.sections.includes(section)) {
			issues.push(`Missing landing section marker: ${section}`);
		}
	}

	for (const capability of contract.landing.required_capabilities) {
		if (!landingOutline.capabilities.includes(capability)) {
			issues.push(`Missing landing capability marker: ${capability}`);
		}
	}

	const docsOutlineSnapshot = {
		source: toPosix(contract.docs.path),
		required_chapters: contract.docs.required_chapters,
		required_template_subheadings: contract.docs.required_template_subheadings,
		chapters: docsChapters,
	};
	const landingOutlineSnapshot = {
		source: toPosix(contract.landing.path),
		required_sections: contract.landing.required_sections,
		required_capabilities: contract.landing.required_capabilities,
		sections: landingOutline.sections,
		capabilities: landingOutline.capabilities,
	};

	const docsSnapshotText = stringifySnapshot(docsOutlineSnapshot);
	const landingSnapshotText = stringifySnapshot(landingOutlineSnapshot);

	if (checkMode) {
		verifySnapshot(contract.snapshots.docs_outline_path, docsSnapshotText, issues);
		verifySnapshot(contract.snapshots.landing_outline_path, landingSnapshotText, issues);
	} else {
		writeSnapshot(contract.snapshots.docs_outline_path, docsSnapshotText);
		writeSnapshot(contract.snapshots.landing_outline_path, landingSnapshotText);
	}

	if (issues.length > 0) {
		for (const issue of issues) {
			fail(issue);
		}
		process.exit(1);
	}

	console.log(`[content-contract] OK${checkMode ? ' (check)' : ' (updated snapshots)'}`);
}

main();
