import assert from 'node:assert/strict';
import test from 'node:test';
import {
	cycleLanguageMode,
	initializeLocalization,
	languageMode,
	listTranslationKeys,
	parseLanguageMode,
	refreshLocaleFromPlatform,
	resolveLocale,
	setLanguagePreference,
	translate,
	type RuntimeEnv,
} from './i18n';
import { get } from 'svelte/store';

function withPlatformLanguages<T>(preferred: string[], run: () => T): T {
	const navigatorDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'navigator');
	const documentDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'document');
	Object.defineProperty(globalThis, 'navigator', {
		configurable: true,
		value: {
			languages: preferred,
			language: preferred[0] ?? 'en-US',
		},
	});
	Object.defineProperty(globalThis, 'document', {
		configurable: true,
		value: {
			documentElement: {
				lang: 'en',
			},
		},
	});
	try {
		return run();
	} finally {
		if (navigatorDescriptor) Object.defineProperty(globalThis, 'navigator', navigatorDescriptor);
		else Reflect.deleteProperty(globalThis, 'navigator');
		if (documentDescriptor) Object.defineProperty(globalThis, 'document', documentDescriptor);
		else Reflect.deleteProperty(globalThis, 'document');
	}
}

function createRuntimeEnv(overrides?: Partial<RuntimeEnv>): RuntimeEnv {
	const storage = new Map<string, string>();
	return {
		fetchImpl: fetch,
		now: () => Date.now(),
		getStorageItem: (key) => storage.get(key) ?? null,
		setStorageItem: (key, value) => {
			storage.set(key, value);
		},
		removeStorageItem: (key) => {
			storage.delete(key);
		},
		getDocumentCookie: () => '',
		getLocation: () => ({
			origin: 'http://localhost:4173',
			protocol: 'http:',
			pathname: '/',
			hash: '',
			search: '',
		}),
		replaceHistoryUrl: () => {},
		getApiUrlOverride: () => null,
		getDesktopInvoke: () => null,
		isTauriRuntime: () => false,
		hasWindow: () => true,
		...overrides,
	};
}

test('parseLanguageMode defaults to system', () => {
	assert.equal(parseLanguageMode('system'), 'system');
	assert.equal(parseLanguageMode('en'), 'en');
	assert.equal(parseLanguageMode('ko'), 'ko');
	assert.equal(parseLanguageMode('fr'), 'system');
	assert.equal(parseLanguageMode(null), 'system');
});

test('resolveLocale follows supported platform languages', () => {
	assert.equal(resolveLocale('system', ['ko-KR', 'en-US']), 'ko');
	assert.equal(resolveLocale('system', ['en-GB']), 'en');
	assert.equal(resolveLocale('ko', ['en-US']), 'ko');
});

test('setLanguagePreference persists and resolves immediately', () => {
	withPlatformLanguages(['ko-KR'], () => {
		const runtime = createRuntimeEnv();
		const resolved = setLanguagePreference('en', runtime);
		assert.equal(resolved, 'en');
		assert.equal(runtime.getStorageItem('opensession_language_mode'), 'en');
		assert.equal(get(languageMode), 'en');
	});
});

test('initializeLocalization reads stored preference and falls back to system locale', () => {
	withPlatformLanguages(['ko-KR'], () => {
		const runtime = createRuntimeEnv();
		runtime.setStorageItem('opensession_language_mode', 'system');
		assert.equal(initializeLocalization(runtime), 'ko');
		assert.equal(get(languageMode), 'system');
	});
});

test('refreshLocaleFromPlatform respects system mode only', () => {
	withPlatformLanguages(['ko-KR'], () => {
		const runtime = createRuntimeEnv();
		initializeLocalization(runtime);
		assert.equal(refreshLocaleFromPlatform(runtime), 'ko');
		setLanguagePreference('en', runtime);
		assert.equal(refreshLocaleFromPlatform(runtime), 'en');
	});
});

test('translate interpolates params and falls back to english keys', () => {
	assert.equal(translate('ko', 'sessionList.header', { total: 3 }), '세션 (3)');
	assert.equal(translate('en', 'language.mode.system'), 'Follow platform');
});

test('translation catalogs stay in sync across locales', () => {
	assert.deepEqual([...listTranslationKeys('en')].sort(), [...listTranslationKeys('ko')].sort());
});

test('cycleLanguageMode rotates through system, en, ko', () => {
	assert.equal(cycleLanguageMode('system'), 'en');
	assert.equal(cycleLanguageMode('en'), 'ko');
	assert.equal(cycleLanguageMode('ko'), 'system');
});
