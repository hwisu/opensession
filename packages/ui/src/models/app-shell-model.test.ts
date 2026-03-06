import assert from 'node:assert/strict';
import test from 'node:test';
import { ApiError } from '../api-internal/errors.ts';
import { createShellModel, createShellModelState } from './app-shell-model.ts';

test('shell model loads auth capabilities and user state', async () => {
	const state = createShellModelState();
	const model = createShellModel(state, {
		getApiCapabilities: async () => ({ auth_enabled: true }),
		verifyAuth: async () => true,
		getSettings: async () => ({
			user_id: 'user-1',
			nickname: 'hwisoo',
			email: 'hwisoo@example.test',
			created_at: '2026-03-06T00:00:00Z',
			oauth_providers: [],
		}),
		authLogout: async () => undefined,
		isAuthenticated: () => true,
		isDesktopRuntime: () => true,
		takeLaunchRoute: async () => null,
		getCurrentLocationPath: () => '/sessions',
		startInterval: () => 1,
		clearInterval: () => undefined,
		navigate: () => undefined,
	});

	await model.loadCapabilities();
	await model.loadUser();

	assert.equal(state.desktopRuntime, true);
	assert.equal(state.authEnabled, true);
	assert.equal(state.hasLocalAuth, true);
	assert.equal(state.user?.nickname, 'hwisoo');
});

test('shell model clears user state and logs out on unauthorized settings fetch', async () => {
	const state = createShellModelState();
	state.authEnabled = true;
	let logoutCalls = 0;
	const model = createShellModel(state, {
		getApiCapabilities: async () => ({ auth_enabled: true }),
		verifyAuth: async () => true,
		getSettings: async () => {
			throw new ApiError(401, '{"message":"unauthorized"}');
		},
		authLogout: async () => {
			logoutCalls += 1;
		},
		isAuthenticated: () => true,
		isDesktopRuntime: () => false,
		takeLaunchRoute: async () => null,
		getCurrentLocationPath: () => '/sessions',
		startInterval: () => 1,
		clearInterval: () => undefined,
		navigate: () => undefined,
	});

	await model.loadUser();

	assert.equal(state.user, null);
	assert.equal(state.hasLocalAuth, false);
	assert.equal(logoutCalls, 1);
});

test('shell model polls desktop launch route and navigates when it changes', async () => {
	const state = createShellModelState();
	let intervalCallback: (() => void) | null = null;
	const navigations: string[] = [];
	let nextRoute: string | null = '/session/next';
	const model = createShellModel(state, {
		getApiCapabilities: async () => ({ auth_enabled: false }),
		verifyAuth: async () => false,
		getSettings: async () => {
			throw new Error('unused');
		},
		authLogout: async () => undefined,
		isAuthenticated: () => false,
		isDesktopRuntime: () => true,
		takeLaunchRoute: async () => {
			const route = nextRoute;
			nextRoute = null;
			return route;
		},
		getCurrentLocationPath: () => '/sessions',
		startInterval: (callback) => {
			intervalCallback = callback;
			return 42;
		},
		clearInterval: () => undefined,
		navigate: (path) => {
			navigations.push(path);
		},
	});

	const dispose = model.startLaunchRoutePolling();
	await Promise.resolve();
	await Promise.resolve();
	assert.deepEqual(navigations, ['/session/next']);

	intervalCallback?.();
	await Promise.resolve();
	assert.deepEqual(navigations, ['/session/next']);

	dispose();
});
