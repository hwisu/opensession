<script lang="ts">
import { goto } from '$app/navigation';
import { page } from '$app/stores';
import { untrack } from 'svelte';
import {
	buildNativeFilterOptions,
	buildUnifiedFilterOptions,
	getApiCapabilities,
	getParsePreviewError,
	type ParseCandidate,
	previewSessionFromGitSource,
	previewSessionFromGithubSource,
	type ParsePreviewResponse,
	type Session,
	type SessionViewMode,
} from '@opensession/ui';
import { ParseSourceBanner, ParserSelectPanel, SessionRenderPage } from '@opensession/ui/components';

type PageState = 'idle' | 'loading' | 'ready' | 'select_parser' | 'error' | 'unsupported';

type SourceRouteState =
	| {
			provider: 'gh';
			owner: string;
			repo: string;
			ref: string;
			path: string;
	  }
	| {
			provider: 'git';
			remote: string;
			ref: string;
			path: string;
	  }
	| {
			provider: 'gl';
			project: string;
			ref: string;
			path: string;
	  };

const VALID_PARSER_HINTS = new Set([
	'hail',
	'codex',
	'claude-code',
	'gemini',
	'amp',
	'cline',
	'cursor',
	'opencode',
]);

let pageState = $state<PageState>('idle');
let errorMessage = $state<string | null>(null);
let preview = $state<ParsePreviewResponse | null>(null);
let parserCandidates = $state<ParseCandidate[]>([]);
let parserHint = $state<string | null>(null);
let viewMode = $state<SessionViewMode>('unified');
let unifiedFilters = $state(new Set<string>());
let nativeFilters = $state(new Set<string>());
let currentRoute = $state<SourceRouteState | null>(null);
let routeQueryEf = $state<string[]>([]);
let routeQueryNf = $state<string[]>([]);
let routeVersion = $state(0);
let lastPreviewKey = $state<string | null>(null);
let lastObservedHref = '';

function parseCsvQuery(value: string | null): string[] {
	if (!value) return [];
	const out: string[] = [];
	const seen = new Set<string>();
	for (const raw of value.split(',')) {
		const trimmed = raw.trim();
		if (!trimmed || seen.has(trimmed)) continue;
		seen.add(trimmed);
		out.push(trimmed);
	}
	return out;
}

function parseViewMode(value: string | null): SessionViewMode {
	return value === 'native' ? 'native' : 'unified';
}

function parseParserHint(value: string | null): string | null {
	if (!value) return null;
	const trimmed = value.trim();
	if (!trimmed || !VALID_PARSER_HINTS.has(trimmed)) return null;
	return trimmed;
}

function decodePathSegments(segments: string[]): string {
	return segments.map((segment) => decodeURIComponent(segment)).join('/');
}

function encodePath(path: string): string {
	return path
		.split('/')
		.map((segment) => encodeURIComponent(segment))
		.join('/');
}

function decodeBase64Url(value: string): string {
	const normalized = value.replace(/-/g, '+').replace(/_/g, '/');
	const padded = normalized + '='.repeat((4 - (normalized.length % 4)) % 4);
	try {
		return decodeURIComponent(escape(atob(padded)));
	} catch {
		throw new Error('Invalid base64url segment');
	}
}

function encodeBase64Url(value: string): string {
	const encoded = btoa(unescape(encodeURIComponent(value)));
	return encoded.replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/g, '');
}

function parseSourceRouteFromParams(): { route: SourceRouteState | null; message?: string } {
	const provider = $page.params.provider;
	const tail = ($page.params.segments ?? '').split('/').filter((segment) => segment.length > 0);

	if (provider === 'gh') {
		if (tail.length < 6 || tail[2] !== 'ref' || tail[4] !== 'path') {
			return {
				route: null,
				message:
					'Invalid source path. Expected /src/gh/<owner>/<repo>/ref/<ref_enc>/path/<path...>.',
			};
		}
		return {
			route: {
				provider: 'gh',
				owner: decodeURIComponent(tail[0]),
				repo: decodeURIComponent(tail[1]),
				ref: decodeURIComponent(tail[3]),
				path: decodePathSegments(tail.slice(5)),
			},
		};
	}

	if (provider === 'git') {
		if (tail.length < 5 || tail[1] !== 'ref' || tail[3] !== 'path') {
			return {
				route: null,
				message:
					'Invalid source path. Expected /src/git/<remote_b64>/ref/<ref_enc>/path/<path...>.',
			};
		}
		const remote = decodeBase64Url(tail[0]);
		return {
			route: {
				provider: 'git',
				remote,
				ref: decodeURIComponent(tail[2]),
				path: decodePathSegments(tail.slice(4)),
			},
		};
	}

	if (provider === 'gl') {
		if (tail.length < 5 || tail[1] !== 'ref' || tail[3] !== 'path') {
			return {
				route: null,
				message:
					'Invalid source path. Expected /src/gl/<project_b64>/ref/<ref_enc>/path/<path...>.',
			};
		}
		return {
			route: {
				provider: 'gl',
				project: decodeBase64Url(tail[0]),
				ref: decodeURIComponent(tail[2]),
				path: decodePathSegments(tail.slice(4)),
			},
		};
	}

	return {
		route: null,
		message: 'Unsupported source provider. Use /src/gh, /src/gl, or /src/git.',
	};
}

function buildRouteBase(route: SourceRouteState): string {
	if (route.provider === 'gh') {
		return `/src/gh/${encodeURIComponent(route.owner)}/${encodeURIComponent(route.repo)}/ref/${encodeURIComponent(route.ref)}/path/${encodePath(route.path)}`;
	}

	if (route.provider === 'gl') {
		return `/src/gl/${encodeBase64Url(route.project)}/ref/${encodeURIComponent(route.ref)}/path/${encodePath(route.path)}`;
	}

	return `/src/git/${encodeBase64Url(route.remote)}/ref/${encodeURIComponent(route.ref)}/path/${encodePath(route.path)}`;
}

function buildStateUrl(route: SourceRouteState): string {
	const params = new URLSearchParams();
	params.set('view', viewMode);
	params.set('ef', Array.from(unifiedFilters).sort().join(','));
	params.set('nf', Array.from(nativeFilters).sort().join(','));
	if (parserHint) {
		params.set('parser_hint', parserHint);
	}
	const query = params.toString();
	const base = buildRouteBase(route);
	return query ? `${base}?${query}` : base;
}

async function syncUrlToState() {
	if (!currentRoute) return;
	const target = buildStateUrl(currentRoute);
	const current = `${$page.url.pathname}${$page.url.search}`;
	if (target === current) return;
	await goto(target, { replaceState: true, keepFocus: true, noScroll: true });
}

function initializeFiltersFromRoute(previewResponse: ParsePreviewResponse) {
	const session = previewResponse.session as Session;
	const allUnified = buildUnifiedFilterOptions(session.events).map((option) => option.key);
	const allNative = buildNativeFilterOptions(session.events).map((option) => option.key);
	const allUnifiedSet = new Set(allUnified);
	const allNativeSet = new Set(allNative);

	if (routeQueryEf.length > 0) {
		const effective = routeQueryEf.filter((key) => allUnifiedSet.has(key));
		unifiedFilters = new Set(effective.length > 0 ? effective : allUnified);
	} else {
		unifiedFilters = allUnifiedSet;
	}

	if (routeQueryNf.length > 0) {
		const effective = routeQueryNf.filter((key) => allNativeSet.has(key));
		nativeFilters = new Set(effective.length > 0 ? effective : allNative);
	} else {
		nativeFilters = allNativeSet;
	}
}

function toggleUnifiedFilter(key: string) {
	const next = new Set(unifiedFilters);
	if (next.has(key)) next.delete(key);
	else next.add(key);
	unifiedFilters = next;
	void syncUrlToState();
}

function toggleNativeFilter(key: string) {
	const next = new Set(nativeFilters);
	if (next.has(key)) next.delete(key);
	else next.add(key);
	nativeFilters = next;
	void syncUrlToState();
}

function changeViewMode(mode: SessionViewMode) {
	viewMode = mode;
	void syncUrlToState();
}

function selectParser(parserId: string) {
	parserHint = parserId;
	void syncUrlToState();
}

async function loadFromRoute() {
	const activeVersion = ++routeVersion;
	pageState = 'loading';
	errorMessage = null;

	const { route, message } = parseSourceRouteFromParams();
	if (!route) {
		pageState = 'error';
		errorMessage = message ?? 'Invalid source route.';
		return;
	}
	currentRoute = route;

	viewMode = parseViewMode($page.url.searchParams.get('view'));
	parserHint = parseParserHint($page.url.searchParams.get('parser_hint'));
	routeQueryEf = parseCsvQuery($page.url.searchParams.get('ef'));
	routeQueryNf = parseCsvQuery($page.url.searchParams.get('nf'));

	const routeKey = `${JSON.stringify(route)}|${parserHint ?? ''}`;
	if (preview && lastPreviewKey === routeKey) {
		initializeFiltersFromRoute(preview);
		pageState = 'ready';
		await syncUrlToState();
		return;
	}

	const capabilities = await getApiCapabilities();
	if (activeVersion !== routeVersion) return;
	if (!capabilities.parse_preview_enabled) {
		pageState = 'unsupported';
		return;
	}

	try {
		const parsed =
			route.provider === 'gh'
				? await previewSessionFromGithubSource({
						owner: route.owner,
						repo: route.repo,
						ref: route.ref,
						path: route.path,
						parser_hint: parserHint ?? undefined,
				  })
				: route.provider === 'gl'
					? await previewSessionFromGitSource({
							remote: `https://gitlab.com/${route.project}`,
							ref: route.ref,
							path: route.path,
							parser_hint: parserHint ?? undefined,
					  })
					: await previewSessionFromGitSource({
							remote: route.remote,
							ref: route.ref,
							path: route.path,
							parser_hint: parserHint ?? undefined,
					  });
		if (activeVersion !== routeVersion) return;

		preview = parsed;
		parserCandidates = [];
		lastPreviewKey = routeKey;
		initializeFiltersFromRoute(parsed);
		pageState = 'ready';
		await syncUrlToState();
	} catch (error) {
		if (activeVersion !== routeVersion) return;
		const parseError = getParsePreviewError(error);
		if (parseError?.code === 'parser_selection_required') {
			parserCandidates = parseError.parser_candidates ?? [];
			errorMessage = parseError.message;
			pageState = 'select_parser';
			return;
		}

		pageState = 'error';
		errorMessage = parseError?.message ?? (error instanceof Error ? error.message : 'Failed to parse source');
	}
}

$effect(() => {
	const href = $page.url.href;
	if (href === lastObservedHref) return;
	lastObservedHref = href;
	untrack(() => {
		void loadFromRoute();
	});
});
</script>

{#if pageState === 'loading' || pageState === 'idle'}
	<div class="py-16 text-center text-xs text-text-muted">Loading source preview...</div>
{:else if pageState === 'unsupported'}
	<div class="mx-auto max-w-3xl border border-border bg-bg-secondary p-6 text-sm text-text-secondary">
		This deployment does not support source parse preview.
	</div>
{:else if pageState === 'select_parser'}
	<div class="mx-auto max-w-3xl space-y-3">
		{#if errorMessage}
			<div class="border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">{errorMessage}</div>
		{/if}
		<ParserSelectPanel
			candidates={parserCandidates}
			parserHint={parserHint}
			loading={false}
			onSelect={selectParser}
		/>
	</div>
{:else if pageState === 'error'}
	<div class="mx-auto max-w-3xl border border-error/30 bg-error/10 px-4 py-3 text-sm text-error">
		{errorMessage ?? 'Failed to load source.'}
	</div>
{:else if preview && currentRoute}
	<div class="space-y-3">
		<ParseSourceBanner source={preview.source} parserUsed={preview.parser_used} warnings={preview.warnings} />
		<SessionRenderPage
			session={preview.session as Session}
			viewMode={viewMode}
			unifiedFilters={unifiedFilters}
			nativeFilters={nativeFilters}
			onViewModeChange={changeViewMode}
			onToggleUnifiedFilter={toggleUnifiedFilter}
			onToggleNativeFilter={toggleNativeFilter}
		/>
	</div>
{:else}
	<div class="py-16 text-center text-xs text-text-muted">No preview data.</div>
{/if}
