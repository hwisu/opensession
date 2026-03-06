import type { SessionViewMode } from './session-filters';

export type SourceRouteState =
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

export const INVALID_GH_PATH_MSG =
	'Invalid source path. Expected /src/gh/<owner>/<repo>/ref/<ref...>/path/<path...>.';
export const INVALID_GIT_PATH_MSG =
	'Invalid source path. Expected /src/git/<remote_b64>/ref/<ref...>/path/<path...>.';
export const INVALID_GL_PATH_MSG =
	'Invalid source path. Expected /src/gl/<project_b64>/ref/<ref...>/path/<path...>.';

export const VALID_PARSER_HINTS = new Set([
	'hail',
	'codex',
	'claude-code',
	'gemini',
	'amp',
	'cline',
	'cursor',
	'opencode',
]);

export function parseCsvQuery(value: string | null): string[] {
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

export function parseViewMode(value: string | null): SessionViewMode {
	if (value === 'native' || value === 'branch') return value;
	return 'unified';
}

export function parseParserHint(value: string | null): string | null {
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

function extractRefAndPath(
	tail: string[],
	refTokenIndex: number,
	pathTokenFromIndex: number,
): { ref: string; path: string } | null {
	const pathTokenIndex = tail.indexOf('path', pathTokenFromIndex);
	if (pathTokenIndex < 0 || pathTokenIndex >= tail.length - 1) return null;

	const refSegments = tail.slice(refTokenIndex + 1, pathTokenIndex);
	const pathSegments = tail.slice(pathTokenIndex + 1);
	if (refSegments.length === 0 || pathSegments.length === 0) return null;

	return {
		ref: decodePathSegments(refSegments),
		path: decodePathSegments(pathSegments),
	};
}

export function parseSourceRoute(params: {
	provider: string | undefined;
	segments: string | undefined;
}): { route: SourceRouteState | null; message?: string } {
	const provider = params.provider ?? '';
	const tail = (params.segments ?? '').split('/').filter((segment) => segment.length > 0);

	if (provider === 'gh') {
		if (tail.length < 6 || tail[2] !== 'ref') {
			return { route: null, message: INVALID_GH_PATH_MSG };
		}
		try {
			const refAndPath = extractRefAndPath(tail, 2, 3);
			if (!refAndPath) return { route: null, message: INVALID_GH_PATH_MSG };
			return {
				route: {
					provider: 'gh',
					owner: decodeURIComponent(tail[0]),
					repo: decodeURIComponent(tail[1]),
					ref: refAndPath.ref,
					path: refAndPath.path,
				},
			};
		} catch {
			return { route: null, message: INVALID_GH_PATH_MSG };
		}
	}

	if (provider === 'git') {
		if (tail.length < 5 || tail[1] !== 'ref') {
			return { route: null, message: INVALID_GIT_PATH_MSG };
		}
		try {
			const refAndPath = extractRefAndPath(tail, 1, 2);
			if (!refAndPath) return { route: null, message: INVALID_GIT_PATH_MSG };
			return {
				route: {
					provider: 'git',
					remote: decodeBase64Url(tail[0]),
					ref: refAndPath.ref,
					path: refAndPath.path,
				},
			};
		} catch {
			return { route: null, message: INVALID_GIT_PATH_MSG };
		}
	}

	if (provider === 'gl') {
		if (tail.length < 5 || tail[1] !== 'ref') {
			return { route: null, message: INVALID_GL_PATH_MSG };
		}
		try {
			const refAndPath = extractRefAndPath(tail, 1, 2);
			if (!refAndPath) return { route: null, message: INVALID_GL_PATH_MSG };
			return {
				route: {
					provider: 'gl',
					project: decodeBase64Url(tail[0]),
					ref: refAndPath.ref,
					path: refAndPath.path,
				},
			};
		} catch {
			return { route: null, message: INVALID_GL_PATH_MSG };
		}
	}

	return {
		route: null,
		message: 'Unsupported source provider. Use /src/gh, /src/gl, or /src/git.',
	};
}

export function buildRouteBase(route: SourceRouteState): string {
	if (route.provider === 'gh') {
		return `/src/gh/${encodeURIComponent(route.owner)}/${encodeURIComponent(route.repo)}/ref/${encodeURIComponent(route.ref)}/path/${encodePath(route.path)}`;
	}

	if (route.provider === 'gl') {
		return `/src/gl/${encodeBase64Url(route.project)}/ref/${encodeURIComponent(route.ref)}/path/${encodePath(route.path)}`;
	}

	return `/src/git/${encodeBase64Url(route.remote)}/ref/${encodeURIComponent(route.ref)}/path/${encodePath(route.path)}`;
}

export function buildStateUrl(args: {
	route: SourceRouteState;
	viewMode: SessionViewMode;
	unifiedFilters: Iterable<string>;
	nativeFilters: Iterable<string>;
	parserHint: string | null;
}): string {
	const params = new URLSearchParams();
	params.set('view', args.viewMode);
	params.set('ef', Array.from(args.unifiedFilters).sort().join(','));
	params.set('nf', Array.from(args.nativeFilters).sort().join(','));
	if (args.parserHint) {
		params.set('parser_hint', args.parserHint);
	}
	const query = params.toString();
	const base = buildRouteBase(args.route);
	return query ? `${base}?${query}` : base;
}
