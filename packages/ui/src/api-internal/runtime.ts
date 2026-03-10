import { Cause, Context, Effect, Exit, Layer } from 'effect';
import {
	createDesktopSessionReadAdapter,
	createUnavailableDesktopSessionReadAdapter,
	createWebSessionReadAdapter,
	type DesktopInvoke,
	type SessionReadAdapter,
} from '../session-adapter';
import { ApiError } from './errors';

declare global {
	interface Window {
		__OPENSESSION_API_URL__?: string;
		__TAURI_INTERNALS__?: unknown;
		__TAURI__?: {
			core?: {
				invoke?: DesktopInvoke;
			};
		};
	}
}

export interface RuntimeLocation {
	origin: string;
	protocol: string;
	pathname: string;
	hash: string;
	search: string;
}

export interface RuntimeEnv {
	fetchImpl: typeof fetch;
	now: () => number;
	getStorageItem: (key: string) => string | null;
	setStorageItem: (key: string, value: string) => void;
	removeStorageItem: (key: string) => void;
	getPreferredLanguages?: () => string[];
	getDocumentCookie: () => string;
	getLocation: () => RuntimeLocation;
	replaceHistoryUrl: (url: string) => void;
	getApiUrlOverride: () => string | null;
	getDesktopInvoke: () => DesktopInvoke | null;
	isTauriRuntime: () => boolean;
	hasWindow: () => boolean;
}

export const RuntimeEnvTag = Context.GenericTag<RuntimeEnv>('opensession/ui/runtime-env');

function isHttpLikeOrigin(origin: string): boolean {
	return origin.startsWith('http://') || origin.startsWith('https://');
}

export function createBrowserRuntimeEnv(): RuntimeEnv {
	return {
		fetchImpl: fetch,
		now: () => Date.now(),
		getStorageItem(key) {
			if (typeof localStorage === 'undefined') return null;
			try {
				return localStorage.getItem(key);
			} catch {
				return null;
			}
		},
		setStorageItem(key, value) {
			if (typeof localStorage === 'undefined') return;
			localStorage.setItem(key, value);
		},
		removeStorageItem(key) {
			if (typeof localStorage === 'undefined') return;
			localStorage.removeItem(key);
		},
		getPreferredLanguages() {
			if (typeof navigator === 'undefined') return [];
			if (Array.isArray(navigator.languages) && navigator.languages.length > 0) {
				return navigator.languages.filter(
					(value): value is string => typeof value === 'string' && value.trim().length > 0,
				);
			}
			if (typeof navigator.language === 'string' && navigator.language.trim().length > 0) {
				return [navigator.language];
			}
			return [];
		},
		getDocumentCookie() {
			if (typeof document === 'undefined') return '';
			return document.cookie ?? '';
		},
		getLocation() {
			if (typeof window === 'undefined') {
				return {
					origin: '',
					protocol: '',
					pathname: '',
					hash: '',
					search: '',
				};
			}
			return {
				origin: window.location.origin,
				protocol: window.location.protocol,
				pathname: window.location.pathname,
				hash: window.location.hash,
				search: window.location.search,
			};
		},
		replaceHistoryUrl(url) {
			if (typeof window === 'undefined') return;
			window.history?.replaceState?.(null, '', url);
		},
		getApiUrlOverride() {
			if (typeof window === 'undefined') return null;
			return window.__OPENSESSION_API_URL__?.trim() || null;
		},
		getDesktopInvoke() {
			if (typeof window === 'undefined') return null;
			const invoke = window.__TAURI__?.core?.invoke;
			return typeof invoke === 'function' ? invoke : null;
		},
		isTauriRuntime() {
			if (typeof window === 'undefined') return false;
			if ('__TAURI_INTERNALS__' in window) return true;
			return window.location.protocol === 'tauri:';
		},
		hasWindow() {
			return typeof window !== 'undefined';
		},
	};
}

export function browserRuntimeLayer(): Layer.Layer<RuntimeEnv> {
	return Layer.succeed(RuntimeEnvTag, createBrowserRuntimeEnv());
}

export function runUiEffect<A, E>(effect: Effect.Effect<A, E, RuntimeEnv>): Promise<A> {
	return Effect.runPromiseExit(Effect.provide(effect, browserRuntimeLayer())).then((exit) => {
		if (Exit.isSuccess(exit)) return exit.value;
		throw Cause.squash(exit.cause);
	});
}

export function readBrowserRuntime(): RuntimeEnv {
	return createBrowserRuntimeEnv();
}

export function hasDesktopApiOverride(runtime: RuntimeEnv): boolean {
	const runtimeOverride = runtime.getApiUrlOverride();
	if (runtimeOverride) return true;
	return Boolean(runtime.getStorageItem('opensession_api_url')?.trim());
}

export function isDesktopLocalRuntime(runtime: RuntimeEnv): boolean {
	return runtime.isTauriRuntime() && !hasDesktopApiOverride(runtime);
}

export function getBaseUrl(runtime: RuntimeEnv): string {
	const runtimeOverride = runtime.getApiUrlOverride();
	if (runtimeOverride) return runtimeOverride;

	const stored = runtime.getStorageItem('opensession_api_url');
	if (stored) return stored;

	const location = runtime.getLocation();
	if (isHttpLikeOrigin(location.origin)) return location.origin;
	if (runtime.isTauriRuntime()) return '';
	if (!location.origin || location.origin === 'null') return '';
	return location.origin;
}

export function getCookieValue(runtime: RuntimeEnv, name: string): string | null {
	const encodedName = `${name}=`;
	const parts = runtime.getDocumentCookie().split(';');
	for (const raw of parts) {
		const trimmed = raw.trim();
		if (trimmed.startsWith(encodedName)) {
			return trimmed.slice(encodedName.length);
		}
	}
	return null;
}

export function getCsrfToken(runtime: RuntimeEnv): string | null {
	return getCookieValue(runtime, 'opensession_csrf_token');
}

export function getAuthHeader(runtime: RuntimeEnv): string | null {
	const apiKey = runtime.getStorageItem('opensession_api_key');
	return apiKey ? `Bearer ${apiKey}` : null;
}

export function setBaseUrl(runtime: RuntimeEnv, url: string) {
	runtime.setStorageItem('opensession_api_url', url);
}

export function isAuthenticated(runtime: RuntimeEnv): boolean {
	return getCsrfToken(runtime) != null;
}

export function desktopHttpApiUnavailable(path: string): ApiError {
	return new ApiError(
		501,
		JSON.stringify({
			code: 'desktop_http_api_unavailable',
			message:
				'HTTP API is unavailable in desktop local runtime. Set OPENSESSION_API_URL to call a remote server.',
			details: { path },
		}),
	);
}

export function assertDesktopHttpApiAvailable(runtime: RuntimeEnv, path: string) {
	if (isDesktopLocalRuntime(runtime)) {
		throw desktopHttpApiUnavailable(path);
	}
}

export function createRuntimeSessionReadAdapter(runtime: RuntimeEnv): SessionReadAdapter {
	const invoke = runtime.getDesktopInvoke();
	if (isDesktopLocalRuntime(runtime)) {
		if (invoke) return createDesktopSessionReadAdapter(invoke);
		return createUnavailableDesktopSessionReadAdapter();
	}
	return createWebSessionReadAdapter({
		baseUrl: getBaseUrl(runtime),
		fetchImpl: runtime.fetchImpl,
		getAuthHeader: async () => getAuthHeader(runtime),
	});
}

export function getOAuthUrl(runtime: RuntimeEnv, provider: string): string {
	if (isDesktopLocalRuntime(runtime)) return '#';
	return `${getBaseUrl(runtime)}/api/auth/oauth/${encodeURIComponent(provider)}`;
}

export function getPreferredLanguages(runtime: RuntimeEnv): string[] {
	if (typeof runtime.getPreferredLanguages === 'function') {
		return runtime.getPreferredLanguages();
	}
	if (!runtime.hasWindow() || typeof navigator === 'undefined') return [];
	if (Array.isArray(navigator.languages) && navigator.languages.length > 0) {
		return navigator.languages.filter(
			(value): value is string => typeof value === 'string' && value.trim().length > 0,
		);
	}
	if (typeof navigator.language === 'string' && navigator.language.trim().length > 0) {
		return [navigator.language];
	}
	return [];
}

export function setDocumentLanguage(language: string) {
	if (typeof document === 'undefined') return;
	if (!document.documentElement) return;
	document.documentElement.lang = language;
}
