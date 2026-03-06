import assert from 'node:assert/strict';
import test from 'node:test';
import { ApiError } from '../api-internal/errors.ts';
import {
	copyTextSurface,
	loadRuntimeSettingsState,
	loadSettingsPageState,
	nextSettingsBackgroundPollDelay,
} from './settings-model.ts';

test('settings page loader marks auth as required when local auth token is missing', async () => {
	const result = await loadSettingsPageState({
		getApiCapabilities: async () => ({ auth_enabled: true }),
		isAuthenticated: () => false,
		getSettings: async () => {
			throw new Error('unused');
		},
		listGitCredentials: async () => {
			throw new Error('unused');
		},
	});

	assert.equal(result.authApiEnabled, true);
	assert.equal(result.authRequired, true);
	assert.equal(result.settings, null);
});

test('runtime settings loader treats desktop 501 as unsupported, not fatal', async () => {
	const result = await loadRuntimeSettingsState({
		getRuntimeSettings: async () => {
			throw new ApiError(501, '{"message":"unsupported"}');
		},
		getLifecycleCleanupStatus: async () => {
			throw new Error('unused');
		},
		getSummaryBatchStatus: async () => {
			throw new Error('unused');
		},
		vectorPreflight: async () => {
			throw new Error('unused');
		},
		vectorIndexStatus: async () => {
			throw new Error('unused');
		},
	});

	assert.equal(result.runtimeSupported, false);
	assert.equal(result.runtimeError, null);
});

test('settings background poll delay uses fast interval for active jobs', () => {
	const delay = nextSettingsBackgroundPollDelay({
		runtimeSupported: true,
		runtimeLifecycleEnabled: true,
		runtimeVectorInstalling: false,
		runtimeVectorPreflight: null,
		runtimeVectorReindexing: true,
		runtimeVectorIndex: null,
		runtimeSummaryBatchRunning: false,
		runtimeSummaryBatchStatus: null,
		runtimeLifecycleStatus: null,
	});

	assert.equal(delay, 1000);
});

test('copy text surface reports clipboard failure without throwing', async () => {
	const result = await copyTextSurface(
		{
			writeText: async () => {
				throw new Error('clipboard unavailable');
			},
		},
		'osk_test_key',
	);

	assert.equal(result, 'Copy failed');
});
