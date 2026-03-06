import assert from 'node:assert/strict';
import test from 'node:test';
import {
	createSourcePreviewModel,
	createSourcePreviewModelState,
} from './source-preview-model.ts';

test('source preview model moves to parser selection state when backend requests it', async () => {
	const state = createSourcePreviewModelState();
	const replaced: string[] = [];
	const model = createSourcePreviewModel(state, {
		getApiCapabilities: async () => ({ parse_preview_enabled: true }),
		previewSessionFromGithubSource: async () => {
			throw new Error('unexpected');
		},
		previewSessionFromGitSource: async () => {
			throw new Error('selection');
		},
		getParsePreviewError: () => ({
			code: 'parser_selection_required',
			message: 'pick one',
			parser_candidates: [{ id: 'codex', label: 'Codex', confidence: 0.9 }],
		}),
		buildUnifiedFilterKeys: () => [],
		buildNativeFilterKeys: () => [],
		replaceStateUrl: async (url) => {
			replaced.push(url);
		},
	});

	await model.loadFromLocation({
		provider: 'git',
		segments: 'aHR0cHM6Ly9naXRodWIuY29tL2h3aXN1L29wZW5zZXNzaW9u/ref/main/path/README.md',
		pathname: '/src/git/x/ref/main/path/README.md',
		search: '',
		href: '/src/git/x/ref/main/path/README.md',
	});

	assert.equal(state.pageState, 'select_parser');
	assert.equal(state.errorMessage, 'pick one');
	assert.equal(state.parserCandidates.length, 1);
	assert.deepEqual(replaced, []);
});

test('source preview model syncs parser hint into route state', async () => {
	const state = createSourcePreviewModelState();
	const replaced: string[] = [];
	const model = createSourcePreviewModel(state, {
		getApiCapabilities: async () => ({ parse_preview_enabled: true }),
		previewSessionFromGithubSource: async () => ({
			source: { kind: 'github', owner: 'hwisu', repo: 'opensession', ref: 'main', path: 'README.md' },
			parser_used: 'codex',
			warnings: [],
			session: {
				version: 'hail-1.0.0',
				session_id: 'session-1',
				agent: { provider: 'openai', model: 'gpt-5', tool: 'codex' },
				context: { tags: [], created_at: '2026-03-05T00:00:00Z', updated_at: '2026-03-05T00:00:00Z' },
				events: [],
				stats: {
					event_count: 0,
					message_count: 0,
					tool_call_count: 0,
					task_count: 0,
					duration_seconds: 0,
					total_input_tokens: 0,
					total_output_tokens: 0,
					user_message_count: 0,
					files_changed: 0,
					lines_added: 0,
					lines_removed: 0,
				},
			},
		}),
		previewSessionFromGitSource: async () => {
			throw new Error('unexpected');
		},
		getParsePreviewError: () => null,
		buildUnifiedFilterKeys: () => ['all'],
		buildNativeFilterKeys: () => ['tool'],
		replaceStateUrl: async (url) => {
			replaced.push(url);
		},
	});

	await model.loadFromLocation({
		provider: 'gh',
		segments: 'hwisu/opensession/ref/main/path/README.md',
		pathname: '/src/gh/hwisu/opensession/ref/main/path/README.md',
		search: '',
		href: '/src/gh/hwisu/opensession/ref/main/path/README.md',
	});
	model.selectParser('codex');

	assert.equal(state.pageState, 'ready');
	assert.equal(state.preview?.parser_used, 'codex');
	assert.match(replaced.at(-1) ?? '', /parser_hint=codex/);
});
