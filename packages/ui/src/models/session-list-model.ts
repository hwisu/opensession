import type { SessionSummary, SessionRepoListResponse, SessionListResponse, TimeRange } from '../types';

export type SessionListCacheEntry = {
	query: string;
	created_at: number;
	total: number;
	sessions: SessionSummary[];
};

export interface SessionListCachePort {
	read: (fingerprint: string) => SessionListCacheEntry | null;
	write: (entry: SessionListCacheEntry) => void;
	clear: () => void;
}

export interface SessionListModelState {
	sessions: SessionSummary[];
	total: number;
	loading: boolean;
	forceRefreshing: boolean;
	error: string | null;
	searchQuery: string;
	toolFilter: string;
	repoFilter: string;
	repoInput: string;
	protocolFilter: string;
	jobIdFilter: string;
	runIdFilter: string;
	stageFilter: string;
	reviewKindFilter: string;
	statusFilter: string;
	timeRange: TimeRange;
	currentPage: number;
	selectedIndex: number;
	renderLimit: number;
	knownRepos: string[];
	hydratedFromQuery: boolean;
	lastResetFingerprint: string | null;
}

export interface SessionListModelDeps {
	listSessions: (params?: {
		search?: string;
		tool?: string;
		git_repo_name?: string;
		protocol?: string;
		job_id?: string;
		run_id?: string;
		stage?: string;
		review_kind?: string;
		status?: string;
		time_range?: string;
		page?: number;
		per_page?: number;
		force_refresh?: boolean;
	}) => Promise<SessionListResponse>;
	listSessionRepos: () => Promise<SessionRepoListResponse>;
	cache: SessionListCachePort;
	getLocationSearch: () => string;
	validToolValues: string[];
	validTimeRanges: ReadonlySet<TimeRange>;
	perPage?: number;
}

const DEFAULT_PER_PAGE = 20;

export function createSessionListModelState(): SessionListModelState {
	return {
		sessions: [],
		total: 0,
		loading: false,
		forceRefreshing: false,
		error: null,
		searchQuery: '',
		toolFilter: '',
		repoFilter: '',
		repoInput: '',
		protocolFilter: '',
		jobIdFilter: '',
		runIdFilter: '',
		stageFilter: '',
		reviewKindFilter: '',
		statusFilter: '',
		timeRange: 'all',
		currentPage: 1,
		selectedIndex: 0,
		renderLimit: DEFAULT_PER_PAGE,
		knownRepos: [],
		hydratedFromQuery: false,
		lastResetFingerprint: null,
	};
}

export function createBrowserSessionListCache(
	key = 'opensession_public_list_cache_v1',
	ttlMs = 30_000,
): SessionListCachePort {
	return {
		read(fingerprint) {
			if (typeof localStorage === 'undefined') return null;
			try {
				const raw = localStorage.getItem(key);
				if (!raw) return null;
				const parsed = JSON.parse(raw) as SessionListCacheEntry;
				if (!parsed || parsed.query !== fingerprint) return null;
				if (Date.now() - parsed.created_at > ttlMs) return null;
				if (!Array.isArray(parsed.sessions)) return null;
				return parsed;
			} catch {
				return null;
			}
		},
		write(entry) {
			if (typeof localStorage === 'undefined') return;
			try {
				localStorage.setItem(key, JSON.stringify(entry));
			} catch {
				// Ignore storage errors.
			}
		},
		clear() {
			if (typeof localStorage === 'undefined') return;
			try {
				localStorage.removeItem(key);
			} catch {
				// Ignore storage errors.
			}
		},
	};
}

function extractRepos(items: SessionSummary[]): string[] {
	const values = new Set<string>();
	for (const session of items) {
		const repo = session.git_repo_name?.trim();
		if (repo) values.add(repo);
	}
	return [...values];
}

function mergeKnownRepos(state: SessionListModelState, items: SessionSummary[]) {
	const merged = new Set(state.knownRepos);
	for (const repo of extractRepos(items)) {
		merged.add(repo);
	}
	state.knownRepos = [...merged].sort((a, b) => a.localeCompare(b));
}

export function createSessionListModel(
	state: SessionListModelState,
	deps: SessionListModelDeps,
) {
	const perPage = deps.perPage ?? DEFAULT_PER_PAGE;
	let fetchRequestId = 0;

	function currentListQueryFingerprint(page: number): string {
		return JSON.stringify({
			search: state.searchQuery || '',
			tool: state.toolFilter || '',
			git_repo_name: state.repoFilter,
			protocol: state.protocolFilter || '',
			job_id: state.jobIdFilter || '',
			run_id: state.runIdFilter || '',
			stage: state.stageFilter || '',
			review_kind: state.reviewKindFilter || '',
			status: state.statusFilter || '',
			time_range: state.timeRange,
			page,
			per_page: perPage,
		});
	}

	function isDefaultPublicFeedQuery(page: number): boolean {
		return (
			page === 1 &&
			state.searchQuery.trim().length === 0 &&
			state.toolFilter.length === 0 &&
			state.repoFilter.length === 0 &&
			state.protocolFilter.length === 0 &&
			state.jobIdFilter.length === 0 &&
			state.runIdFilter.length === 0 &&
			state.stageFilter.length === 0 &&
			state.reviewKindFilter.length === 0 &&
			state.statusFilter.length === 0 &&
			state.timeRange === 'all'
		);
	}

	function hydrateFiltersFromQuery() {
		const params = new URLSearchParams(deps.getLocationSearch());

		const repoFromQuery = params.get('git_repo_name')?.trim();
		if (repoFromQuery) {
			state.repoFilter = repoFromQuery;
			state.repoInput = repoFromQuery;
		}

		const searchFromQuery = params.get('search')?.trim();
		if (searchFromQuery) {
			state.searchQuery = searchFromQuery;
		}

		const toolFromQuery = params.get('tool')?.trim();
		if (toolFromQuery && deps.validToolValues.includes(toolFromQuery)) {
			state.toolFilter = toolFromQuery;
		}

		const protocolFromQuery = params.get('protocol')?.trim();
		if (protocolFromQuery) state.protocolFilter = protocolFromQuery;

		const jobIdFromQuery = params.get('job_id')?.trim();
		if (jobIdFromQuery) state.jobIdFilter = jobIdFromQuery;

		const runIdFromQuery = params.get('run_id')?.trim();
		if (runIdFromQuery) state.runIdFilter = runIdFromQuery;

		const stageFromQuery = params.get('stage')?.trim();
		if (stageFromQuery) state.stageFilter = stageFromQuery;

		const reviewKindFromQuery = params.get('review_kind')?.trim();
		if (reviewKindFromQuery) state.reviewKindFilter = reviewKindFromQuery;

		const statusFromQuery = params.get('status')?.trim();
		if (statusFromQuery) state.statusFilter = statusFromQuery;

		const rangeFromQuery = params.get('time_range')?.trim() as TimeRange | undefined;
		if (rangeFromQuery && deps.validTimeRanges.has(rangeFromQuery)) {
			state.timeRange = rangeFromQuery;
		}
	}

	async function fetchSessions(reset = false, opts: { force?: boolean } = {}) {
		const forceRefresh = opts.force === true;
		const requestId = ++fetchRequestId;
		const targetPage = reset ? 1 : state.currentPage;
		const preserveVisibleSessions = reset && forceRefresh && state.sessions.length > 0;
		state.forceRefreshing = forceRefresh;

		let usedWarmCache = false;
		const fingerprint = currentListQueryFingerprint(targetPage);
		if (
			reset &&
			!forceRefresh &&
			state.lastResetFingerprint === fingerprint &&
			state.sessions.length > 0
		) {
			return;
		}
		if (reset) {
			state.currentPage = targetPage;
			if (!preserveVisibleSessions) {
				state.sessions = [];
				state.selectedIndex = 0;
				state.renderLimit = perPage;
			}
		}
		if (reset && !forceRefresh && isDefaultPublicFeedQuery(targetPage)) {
			const cached = deps.cache.read(fingerprint);
			if (cached) {
				state.sessions = cached.sessions;
				state.total = cached.total;
				state.renderLimit = Math.max(perPage, Math.min(cached.sessions.length, perPage));
				mergeKnownRepos(state, cached.sessions);
				usedWarmCache = true;
			}
		}

		state.loading = !usedWarmCache && !preserveVisibleSessions;
		state.error = null;
		try {
			const response = await deps.listSessions({
				search: state.searchQuery || undefined,
				tool: state.toolFilter || undefined,
				git_repo_name: state.repoFilter || undefined,
				protocol: state.protocolFilter || undefined,
				job_id: state.jobIdFilter || undefined,
				run_id: state.runIdFilter || undefined,
				stage: state.stageFilter || undefined,
				review_kind: state.reviewKindFilter || undefined,
				status: state.statusFilter || undefined,
				time_range: state.timeRange !== 'all' ? state.timeRange : undefined,
				page: targetPage,
				per_page: perPage,
				force_refresh: forceRefresh,
			});
			if (requestId !== fetchRequestId) return;
			if (reset) {
				state.sessions = response.sessions;
				state.renderLimit = Math.max(perPage, Math.min(response.sessions.length, perPage));
				state.lastResetFingerprint = fingerprint;
			} else {
				state.sessions = [...state.sessions, ...response.sessions];
			}
			state.total = response.total;
			mergeKnownRepos(state, response.sessions);
			if (reset && isDefaultPublicFeedQuery(targetPage)) {
				deps.cache.write({
					query: fingerprint,
					created_at: Date.now(),
					total: response.total,
					sessions: response.sessions,
				});
			}
		} catch (error) {
			if (requestId !== fetchRequestId) return;
			state.error = error instanceof Error ? error.message : 'Failed to load sessions';
		} finally {
			if (requestId === fetchRequestId) {
				state.loading = false;
			}
			if (forceRefresh) {
				state.forceRefreshing = false;
			}
		}
	}

	async function fetchKnownRepos() {
		try {
			const response = await deps.listSessionRepos();
			state.knownRepos = [...response.repos].sort((a, b) => a.localeCompare(b));
		} catch {
			// Keep fallback behavior.
		}
	}

	async function loadInitial() {
		if (!state.hydratedFromQuery) {
			hydrateFiltersFromQuery();
			state.hydratedFromQuery = true;
		}
		await fetchKnownRepos();
		await fetchSessions(true);
	}

	function handleSearch() {
		return fetchSessions(true);
	}

	function forceRefreshSessions() {
		deps.cache.clear();
		void fetchKnownRepos();
		return fetchSessions(true, { force: true });
	}

	function loadMore() {
		state.currentPage += 1;
		return fetchSessions(false);
	}

	function renderMore() {
		state.renderLimit = Math.min(state.renderLimit + perPage, state.sessions.length);
	}

	function applyRepoFilter(nextValue: string) {
		const normalized = nextValue.trim();
		state.repoFilter = normalized;
		state.repoInput = normalized;
		return fetchSessions(true);
	}

	function clearRepoFilter() {
		return applyRepoFilter('');
	}

	return {
		loadInitial,
		fetchSessions,
		fetchKnownRepos,
		handleSearch,
		forceRefreshSessions,
		loadMore,
		renderMore,
		applyRepoFilter,
		clearRepoFilter,
	};
}
