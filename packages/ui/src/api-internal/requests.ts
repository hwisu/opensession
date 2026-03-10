import { Effect } from 'effect';
import { ApiError } from './errors';
import {
	assertDesktopHttpApiAvailable,
	getAuthHeader,
	getBaseUrl,
	getCsrfToken,
	type RuntimeEnv,
	RuntimeEnvTag,
} from './runtime';

type RequestEffectOptions = RequestInit & {
	includeAuthHeader?: boolean;
};

export function requestEffect<T>(
	path: string,
	options: RequestEffectOptions = {},
): Effect.Effect<T, unknown, RuntimeEnv> {
	return Effect.gen(function* () {
		const runtime = yield* RuntimeEnvTag;
		assertDesktopHttpApiAvailable(runtime, path);
		const url = `${getBaseUrl(runtime)}${path}`;
		const method = (options.method ?? 'GET').toUpperCase();
		const needsCsrf = method !== 'GET' && method !== 'HEAD' && method !== 'OPTIONS';
		const includeAuthHeader = options.includeAuthHeader !== false;
		const headers: Record<string, string> = {
			'Content-Type': 'application/json',
			...(options.headers as Record<string, string>),
		};

		if (includeAuthHeader) {
			const auth = getAuthHeader(runtime);
			if (auth) headers.Authorization = auth;
		}
		if (needsCsrf) {
			const csrf = getCsrfToken(runtime);
			if (csrf) headers['X-CSRF-Token'] = csrf;
		}

		const response = yield* Effect.tryPromise(() =>
			runtime.fetchImpl(url, {
				...options,
				headers,
				credentials: 'include',
			}),
		);

		if (!response.ok) {
			const body = yield* Effect.tryPromise(() => response.text());
			return yield* Effect.fail(new ApiError(response.status, body));
		}

		if (response.status === 204) {
			return undefined as T;
		}

		const contentType = response.headers.get('content-type') || '';
		if (!contentType.includes('application/json')) {
			return undefined as T;
		}

		const text = yield* Effect.tryPromise(() => response.text());
		if (!text.trim()) {
			return undefined as T;
		}

		return JSON.parse(text) as T;
	});
}
