import type { ParseCandidate, ParsePreviewResponse, Session } from '../types';
import type { SessionViewMode } from '../session-filters';
import {
	buildStateUrl,
	parseCsvQuery,
	parseParserHint,
	parseSourceRoute,
	parseViewMode,
	type SourceRouteState,
} from '../source-route';

type PageState = 'idle' | 'loading' | 'ready' | 'select_parser' | 'error' | 'unsupported';

export interface SourcePreviewLocation {
	provider: string | undefined;
	segments: string | undefined;
	pathname: string;
	search: string;
	href: string;
}

export interface SourcePreviewModelState {
	pageState: PageState;
	errorMessage: string | null;
	preview: ParsePreviewResponse | null;
	parserCandidates: ParseCandidate[];
	parserHint: string | null;
	viewMode: SessionViewMode;
	unifiedFilters: Set<string>;
	nativeFilters: Set<string>;
	currentRoute: SourceRouteState | null;
	routeQueryEf: string[];
	routeQueryNf: string[];
	lastPreviewKey: string | null;
}

export interface SourcePreviewModelDeps {
	getApiCapabilities: () => Promise<{ parse_preview_enabled: boolean }>;
	previewSessionFromGithubSource: (params: {
		owner: string;
		repo: string;
		ref: string;
		path: string;
		parser_hint?: string;
	}) => Promise<ParsePreviewResponse>;
	previewSessionFromGitSource: (params: {
		remote: string;
		ref: string;
		path: string;
		parser_hint?: string;
	}) => Promise<ParsePreviewResponse>;
	getParsePreviewError: (error: unknown) => {
		code?: string;
		message?: string;
		parser_candidates?: ParseCandidate[];
	} | null;
	buildUnifiedFilterKeys: (session: Session) => string[];
	buildNativeFilterKeys: (session: Session) => string[];
	replaceStateUrl: (url: string) => Promise<void>;
}

export function createSourcePreviewModelState(): SourcePreviewModelState {
	return {
		pageState: 'idle',
		errorMessage: null,
		preview: null,
		parserCandidates: [],
		parserHint: null,
		viewMode: 'unified',
		unifiedFilters: new Set<string>(),
		nativeFilters: new Set<string>(),
		currentRoute: null,
		routeQueryEf: [],
		routeQueryNf: [],
		lastPreviewKey: null,
	};
}

export function createSourcePreviewModel(
	state: SourcePreviewModelState,
	deps: SourcePreviewModelDeps,
) {
	let routeVersion = 0;
	let lastObservedHref = '';
	let currentLocation: SourcePreviewLocation | null = null;

	async function syncUrlToState() {
		if (!state.currentRoute || !currentLocation) return;
		const target = buildStateUrl({
			route: state.currentRoute,
			viewMode: state.viewMode,
			unifiedFilters: state.unifiedFilters,
			nativeFilters: state.nativeFilters,
			parserHint: state.parserHint,
		});
		const current = `${currentLocation.pathname}${currentLocation.search}`;
		if (target === current) return;
		await deps.replaceStateUrl(target);
	}

	function initializeFiltersFromRoute(previewResponse: ParsePreviewResponse) {
		const session = previewResponse.session as Session;
		const allUnified = deps.buildUnifiedFilterKeys(session);
		const allNative = deps.buildNativeFilterKeys(session);
		const allUnifiedSet = new Set(allUnified);
		const allNativeSet = new Set(allNative);

		if (state.routeQueryEf.length > 0) {
			const effective = state.routeQueryEf.filter((key) => allUnifiedSet.has(key));
			state.unifiedFilters = new Set(effective.length > 0 ? effective : allUnified);
		} else {
			state.unifiedFilters = allUnifiedSet;
		}

		if (state.routeQueryNf.length > 0) {
			const effective = state.routeQueryNf.filter((key) => allNativeSet.has(key));
			state.nativeFilters = new Set(effective.length > 0 ? effective : allNative);
		} else {
			state.nativeFilters = allNativeSet;
		}
	}

	async function loadFromLocation(location: SourcePreviewLocation) {
		if (location.href === lastObservedHref) return;
		lastObservedHref = location.href;
		currentLocation = location;

		const activeVersion = ++routeVersion;
		state.pageState = 'loading';
		state.errorMessage = null;

		const { route, message } = parseSourceRoute({
			provider: location.provider,
			segments: location.segments,
		});
		if (!route) {
			state.pageState = 'error';
			state.errorMessage = message ?? 'Invalid source route.';
			return;
		}
		state.currentRoute = route;

		const searchParams = new URLSearchParams(location.search);
		state.viewMode = parseViewMode(searchParams.get('view'));
		state.parserHint = parseParserHint(searchParams.get('parser_hint'));
		state.routeQueryEf = parseCsvQuery(searchParams.get('ef'));
		state.routeQueryNf = parseCsvQuery(searchParams.get('nf'));

		const routeKey = `${JSON.stringify(route)}|${state.parserHint ?? ''}`;
		if (state.preview && state.lastPreviewKey === routeKey) {
			initializeFiltersFromRoute(state.preview);
			state.pageState = 'ready';
			await syncUrlToState();
			return;
		}

		const capabilities = await deps.getApiCapabilities();
		if (activeVersion !== routeVersion) return;
		if (!capabilities.parse_preview_enabled) {
			state.pageState = 'unsupported';
			return;
		}

		try {
			const parsed =
				route.provider === 'gh'
					? await deps.previewSessionFromGithubSource({
							owner: route.owner,
							repo: route.repo,
							ref: route.ref,
							path: route.path,
							parser_hint: state.parserHint ?? undefined,
					  })
					: route.provider === 'gl'
						? await deps.previewSessionFromGitSource({
								remote: `https://gitlab.com/${route.project}`,
								ref: route.ref,
								path: route.path,
								parser_hint: state.parserHint ?? undefined,
						  })
						: await deps.previewSessionFromGitSource({
								remote: route.remote,
								ref: route.ref,
								path: route.path,
								parser_hint: state.parserHint ?? undefined,
						  });
			if (activeVersion !== routeVersion) return;

			state.preview = parsed;
			state.parserCandidates = [];
			state.lastPreviewKey = routeKey;
			initializeFiltersFromRoute(parsed);
			state.pageState = 'ready';
			await syncUrlToState();
		} catch (error) {
			if (activeVersion !== routeVersion) return;
			const parseError = deps.getParsePreviewError(error);
			if (parseError?.code === 'parser_selection_required') {
				state.parserCandidates = parseError.parser_candidates ?? [];
				state.errorMessage = parseError.message ?? 'Parser selection required.';
				state.pageState = 'select_parser';
				return;
			}

			state.pageState = 'error';
			state.errorMessage =
				parseError?.message ?? (error instanceof Error ? error.message : 'Failed to parse source');
		}
	}

	function toggleUnifiedFilter(key: string) {
		const next = new Set(state.unifiedFilters);
		if (next.has(key)) next.delete(key);
		else next.add(key);
		state.unifiedFilters = next;
		void syncUrlToState();
	}

	function toggleNativeFilter(key: string) {
		const next = new Set(state.nativeFilters);
		if (next.has(key)) next.delete(key);
		else next.add(key);
		state.nativeFilters = next;
		void syncUrlToState();
	}

	function changeViewMode(mode: SessionViewMode) {
		state.viewMode = mode;
		void syncUrlToState();
	}

	function selectParser(parserId: string) {
		state.parserHint = parserId;
		void syncUrlToState();
	}

	return {
		loadFromLocation,
		toggleUnifiedFilter,
		toggleNativeFilter,
		changeViewMode,
		selectParser,
	};
}
