<script lang="ts">
import { goto } from '$app/navigation';
import { page } from '$app/stores';
import {
	buildNativeFilterOptions,
	buildUnifiedFilterOptions,
	getApiCapabilities,
	getParsePreviewError,
	type ParseCandidate,
	previewSessionFromGithubSource,
	type ParsePreviewResponse,
	type Session,
	type SessionViewMode,
} from '@opensession/ui';
import { ParseSourceBanner, ParserSelectPanel, SessionRenderPage } from '@opensession/ui/components';

type PageState = 'idle' | 'loading' | 'ready' | 'select_parser' | 'error' | 'unsupported';

type NormalizedRoute = {
	owner: string;
	repo: string;
	ref: string;
	path: string;
	basePath: string;
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
let currentRoute = $state<NormalizedRoute | null>(null);
let routeQueryEf = $state<string[]>([]);
let routeQueryNf = $state<string[]>([]);
let routeVersion = $state(0);
let lastPreviewKey = $state<string | null>(null);

function safeDecode(value: string): string | null {
	try {
		return decodeURIComponent(value);
	} catch {
		return null;
	}
}

function normalizePathSegments(rawPath: string): string[] | null {
	const parts = rawPath.split('/');
	if (parts.length === 0) return null;

	const normalized: string[] = [];
	for (const part of parts) {
		const decoded = safeDecode(part);
		if (decoded == null) return null;
		if (decoded.length === 0 || decoded === '.' || decoded === '..') return null;
		normalized.push(decoded);
	}
	return normalized;
}

function normalizeRouteFromUrl(): { route: NormalizedRoute | null; message?: string } {
	const ownerParam = $page.params.owner;
	const repoParam = $page.params.repo;
	const refParam = $page.params.ref;
	const pathParam = $page.params.path;

	if (!ownerParam || !repoParam || !refParam || !pathParam) {
		return { route: null, message: 'Missing required GitHub route parameters.' };
	}

	const owner = safeDecode(ownerParam)?.trim();
	const repo = safeDecode(repoParam)?.trim();
	const ref = safeDecode(refParam)?.trim();
	if (!owner || !repo || !ref) {
		return { route: null, message: 'Invalid owner/repo/ref parameter encoding.' };
	}

	const pathSegments = normalizePathSegments(pathParam);
	if (!pathSegments) {
		return { route: null, message: "Invalid path. Empty, '.' and '..' segments are not allowed." };
	}

	const encodedPath = pathSegments.map((segment) => encodeURIComponent(segment)).join('/');
	const basePath = `/gh/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}/${encodeURIComponent(ref)}/${encodedPath}`;

	return {
		route: {
			owner,
			repo,
			ref,
			path: pathSegments.join('/'),
			basePath,
		},
	};
}

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
	if (!trimmed) return null;
	if (!VALID_PARSER_HINTS.has(trimmed)) return null;
	return trimmed;
}

function buildStateUrl(route: NormalizedRoute): string {
	const params = new URLSearchParams();
	params.set('view', viewMode);
	params.set('ef', Array.from(unifiedFilters).sort().join(','));
	params.set('nf', Array.from(nativeFilters).sort().join(','));
	if (parserHint) {
		params.set('parser_hint', parserHint);
	}
	return `${route.basePath}?${params.toString()}`;
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
	const allUnified = buildUnifiedFilterOptions(session.events).map(
		(option) => option.key,
	);
	const allNative = buildNativeFilterOptions(session.events).map(
		(option) => option.key,
	);
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

	const { route, message } = normalizeRouteFromUrl();
	if (!route) {
		pageState = 'error';
		errorMessage = message ?? 'Invalid GitHub source route.';
		return;
	}
	currentRoute = route;

	viewMode = parseViewMode($page.url.searchParams.get('view'));
	parserHint = parseParserHint($page.url.searchParams.get('parser_hint'));
	routeQueryEf = parseCsvQuery($page.url.searchParams.get('ef'));
	routeQueryNf = parseCsvQuery($page.url.searchParams.get('nf'));

	const routeKey = `${route.owner}/${route.repo}/${route.ref}/${route.path}|${parserHint ?? ''}`;
	const currentCanonicalBase = `${$page.url.pathname}`.split('?')[0];
	if (currentCanonicalBase !== route.basePath) {
		unifiedFilters = new Set(routeQueryEf);
		nativeFilters = new Set(routeQueryNf);
		await syncUrlToState();
		return;
	}

	if (preview && lastPreviewKey === routeKey) {
		initializeFiltersFromRoute(preview);
		pageState = 'ready';
		await syncUrlToState();
		return;
	}

	const capabilities = await getApiCapabilities();
	if (activeVersion !== routeVersion) return;
	if (!capabilities.gh_share_enabled) {
		pageState = 'unsupported';
		return;
	}

	try {
		const parsed = await previewSessionFromGithubSource({
			owner: route.owner,
			repo: route.repo,
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
	void $page.url.href;
	void loadFromRoute();
});
</script>

{#if pageState === 'loading' || pageState === 'idle'}
	<div class="py-16 text-center text-xs text-text-muted">Loading source preview...</div>
{:else if pageState === 'unsupported'}
	<div class="mx-auto max-w-3xl border border-border bg-bg-secondary p-6 text-sm text-text-secondary">
		This deployment is read-only and does not support GitHub share preview.
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
{:else if pageState === 'ready' && preview}
	<div class="space-y-2">
		<ParseSourceBanner
			source={preview.source}
			parserUsed={preview.parser_used}
			warnings={preview.warnings}
		/>
		<SessionRenderPage
			session={preview.session as Session}
			detail={null}
			{viewMode}
			nativeAdapter={preview.native_adapter}
			{unifiedFilters}
			{nativeFilters}
			onViewModeChange={changeViewMode}
			onToggleUnifiedFilter={toggleUnifiedFilter}
			onToggleNativeFilter={toggleNativeFilter}
		/>
	</div>
{/if}
