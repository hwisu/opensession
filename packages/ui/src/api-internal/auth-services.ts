import { Effect } from 'effect';
import type {
	AuthProvidersResponse,
	AuthTokenResponse,
	CapabilitiesResponse,
	GitCredentialSummary,
	IssueApiKeyResponse,
	ListGitCredentialsResponse,
	UserSettings,
} from '../types';
import { requestEffect } from './requests';
import {
	getBaseUrl,
	getCsrfToken,
	getOAuthUrl,
	isAuthenticated,
	isDesktopLocalRuntime,
	type RuntimeEnv,
	RuntimeEnvTag,
} from './runtime';
import { ApiError } from './errors';
import { getApiCapabilitiesEffect, getAuthProvidersEffect } from './session-services';

export function isAuthenticatedEffect(): Effect.Effect<boolean, never, RuntimeEnv> {
	return Effect.gen(function* () {
		const runtime = yield* RuntimeEnvTag;
		return isAuthenticated(runtime);
	});
}

export function verifyAuthEffect(): Effect.Effect<boolean, unknown, RuntimeEnv> {
	return Effect.gen(function* () {
		try {
			yield* requestEffect<unknown>('/api/auth/verify', { method: 'POST' });
			return true;
		} catch (error) {
			if (error instanceof ApiError && (error.status === 401 || error.status === 403)) {
				const refreshed = yield* tryRefreshTokenEffect();
				if (!refreshed) return false;
				try {
					yield* requestEffect<unknown>('/api/auth/verify', { method: 'POST' });
					return true;
				} catch {
					return false;
				}
			}
			return false;
		}
	});
}

export function getSettingsEffect(): Effect.Effect<UserSettings, unknown, RuntimeEnv> {
	return requestEffect<UserSettings>('/api/auth/me');
}

export function issueApiKeyEffect(): Effect.Effect<IssueApiKeyResponse, unknown, RuntimeEnv> {
	return requestEffect<IssueApiKeyResponse>('/api/auth/api-keys/issue', {
		method: 'POST',
	});
}

export function listGitCredentialsEffect(): Effect.Effect<
	GitCredentialSummary[],
	unknown,
	RuntimeEnv
> {
	return Effect.gen(function* () {
		const response = yield* requestEffect<ListGitCredentialsResponse>('/api/auth/git-credentials');
		return response.credentials ?? [];
	});
}

export function createGitCredentialEffect(params: {
	label: string;
	host: string;
	path_prefix?: string | null;
	header_name: string;
	header_value: string;
}): Effect.Effect<GitCredentialSummary, unknown, RuntimeEnv> {
	return requestEffect<GitCredentialSummary>('/api/auth/git-credentials', {
		method: 'POST',
		body: JSON.stringify({
			label: params.label,
			host: params.host,
			path_prefix: params.path_prefix ?? null,
			header_name: params.header_name,
			header_value: params.header_value,
		}),
	});
}

export function deleteGitCredentialEffect(id: string): Effect.Effect<void, unknown, RuntimeEnv> {
	return requestEffect<void>(`/api/auth/git-credentials/${encodeURIComponent(id)}`, {
		method: 'DELETE',
	});
}

export function authRegisterEffect(
	email: string,
	password: string,
	nickname: string,
): Effect.Effect<AuthTokenResponse, unknown, RuntimeEnv> {
	return requestEffect<AuthTokenResponse>('/api/auth/register', {
		method: 'POST',
		body: JSON.stringify({ email, password, nickname }),
		includeAuthHeader: false,
	});
}

export function authLoginEffect(
	email: string,
	password: string,
): Effect.Effect<AuthTokenResponse, unknown, RuntimeEnv> {
	return requestEffect<AuthTokenResponse>('/api/auth/login', {
		method: 'POST',
		body: JSON.stringify({ email, password }),
		includeAuthHeader: false,
	});
}

function tryRefreshTokenEffect(): Effect.Effect<boolean, unknown, RuntimeEnv> {
	return Effect.gen(function* () {
		const runtime = yield* RuntimeEnvTag;
		if (isDesktopLocalRuntime(runtime)) return false;
		try {
			const url = `${getBaseUrl(runtime)}/api/auth/refresh`;
			const headers: Record<string, string> = { 'Content-Type': 'application/json' };
			const csrf = getCsrfToken(runtime);
			if (csrf) headers['X-CSRF-Token'] = csrf;
			const response = yield* Effect.tryPromise(() =>
				runtime.fetchImpl(url, {
					method: 'POST',
					headers,
					credentials: 'include',
				}),
			);
			if (!response.ok) return false;
			yield* Effect.tryPromise(() => response.json());
			return true;
		} catch {
			return false;
		}
	});
}

export function authLogoutEffect(): Effect.Effect<void, never, RuntimeEnv> {
	return Effect.catchAll(
		requestEffect('/api/auth/logout', {
			method: 'POST',
		}),
		() => Effect.void,
	);
}

export function getAuthProvidersSafeEffect(): Effect.Effect<AuthProvidersResponse, never, RuntimeEnv> {
	return Effect.catchAll(getAuthProvidersEffect(), () =>
		Effect.succeed({ email_password: false, oauth: [] }),
	);
}

export function getApiCapabilitiesSafeEffect(): Effect.Effect<
	CapabilitiesResponse,
	never,
	RuntimeEnv
> {
	return Effect.catchAll(getApiCapabilitiesEffect(), () =>
		Effect.succeed({
			auth_enabled: false,
			parse_preview_enabled: false,
			register_targets: [],
			share_modes: [],
		}),
	);
}

export function getOAuthUrlEffect(provider: string): Effect.Effect<string, never, RuntimeEnv> {
	return Effect.gen(function* () {
		const runtime = yield* RuntimeEnvTag;
		return getOAuthUrl(runtime, provider);
	});
}

export function handleAuthCallbackEffect(): Effect.Effect<boolean, unknown, RuntimeEnv> {
	return Effect.gen(function* () {
		const runtime = yield* RuntimeEnvTag;
		if (!runtime.hasWindow()) return false;
		const location = runtime.getLocation();
		if (location.hash) {
			runtime.replaceHistoryUrl(location.pathname);
		}
		try {
			yield* requestEffect<unknown>('/api/auth/verify', { method: 'POST' });
			return true;
		} catch {
			return false;
		}
	});
}
