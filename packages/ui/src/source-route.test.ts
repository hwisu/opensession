import assert from 'node:assert/strict';
import test from 'node:test';
import {
	buildStateUrl,
	parseParserHint,
	parseSourceRoute,
	parseViewMode,
} from './source-route.ts';

test('parseSourceRoute decodes git route payloads', () => {
	const route = parseSourceRoute({
		provider: 'git',
		segments: 'aHR0cHM6Ly9naXRodWIuY29tL2h3aXN1L29wZW5zZXNzaW9u/ref/main/path/crates%2Fcore%2Fsrc%2Flib.rs',
	});

	assert.equal(route.route?.provider, 'git');
	assert.equal(route.route?.remote, 'https://github.com/hwisu/opensession');
	assert.equal(route.route?.ref, 'main');
	assert.equal(route.route?.path, 'crates/core/src/lib.rs');
});

test('buildStateUrl preserves view/filter/parser state', () => {
	const url = buildStateUrl({
		route: {
			provider: 'gh',
			owner: 'hwisu',
			repo: 'opensession',
			ref: 'feat/refactor',
			path: 'packages/ui/src/api.ts',
		},
		viewMode: 'native',
		unifiedFilters: ['all', 'tool'],
		nativeFilters: ['user'],
		parserHint: 'codex',
	});

	assert.match(url, /^\/src\/gh\/hwisu\/opensession\/ref\/feat%2Frefactor\/path\/packages\/ui\/src\/api.ts\?/);
	assert.match(url, /view=native/);
	assert.match(url, /ef=all%2Ctool/);
	assert.match(url, /nf=user/);
	assert.match(url, /parser_hint=codex/);
});

test('route helpers normalize invalid parser/view values', () => {
	assert.equal(parseViewMode('weird'), 'unified');
	assert.equal(parseViewMode('branch'), 'branch');
	assert.equal(parseParserHint('invalid-parser'), null);
	assert.equal(parseParserHint('codex'), 'codex');
});
