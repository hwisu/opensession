import { Effect } from 'effect';
import type {
	ParsePreviewErrorResponse,
	ParsePreviewRequest,
	ParsePreviewResponse,
	ParseSource,
} from '../types';
import { ApiError, PreviewApiError } from './errors';
import { requestEffect } from './requests';
import type { RuntimeEnv } from './runtime';

function postParsePreviewEffect(
	request: ParsePreviewRequest,
): Effect.Effect<ParsePreviewResponse, unknown, RuntimeEnv> {
	return Effect.gen(function* () {
		try {
			return yield* requestEffect<ParsePreviewResponse>('/api/parse/preview', {
				method: 'POST',
				body: JSON.stringify(request),
			});
		} catch (error) {
			if (error instanceof ApiError) {
				let parsed: ParsePreviewErrorResponse | null = null;
				try {
					parsed = JSON.parse(error.body) as ParsePreviewErrorResponse;
				} catch {
					parsed = null;
				}
				if (parsed && typeof parsed.code === 'string' && typeof parsed.message === 'string') {
					throw new PreviewApiError(error.status, parsed);
				}
			}
			throw error;
		}
	});
}

export function previewSessionFromGithubSourceEffect(params: {
	owner: string;
	repo: string;
	ref: string;
	path: string;
	parser_hint?: string;
}): Effect.Effect<ParsePreviewResponse, unknown, RuntimeEnv> {
	const source: ParseSource = {
		kind: 'github',
		owner: params.owner,
		repo: params.repo,
		ref: params.ref,
		path: params.path,
	};
	return postParsePreviewEffect({
		source,
		parser_hint: params.parser_hint ?? null,
	});
}

export function previewSessionFromGitSourceEffect(params: {
	remote: string;
	ref: string;
	path: string;
	parser_hint?: string;
}): Effect.Effect<ParsePreviewResponse, unknown, RuntimeEnv> {
	const source: ParseSource = {
		kind: 'git',
		remote: params.remote,
		ref: params.ref,
		path: params.path,
	};
	return postParsePreviewEffect({
		source,
		parser_hint: params.parser_hint ?? null,
	});
}

export function previewSessionFromInlineSourceEffect(params: {
	filename: string;
	content_base64: string;
	parser_hint?: string;
}): Effect.Effect<ParsePreviewResponse, unknown, RuntimeEnv> {
	const source: ParseSource = {
		kind: 'inline',
		filename: params.filename,
		content_base64: params.content_base64,
	};
	return postParsePreviewEffect({
		source,
		parser_hint: params.parser_hint ?? null,
	});
}

export function getParsePreviewError(error: unknown): ParsePreviewErrorResponse | null {
	if (error instanceof PreviewApiError) return error.payload;
	if (error instanceof ApiError) {
		try {
			const parsed = JSON.parse(error.body) as ParsePreviewErrorResponse;
			if (typeof parsed.code === 'string' && typeof parsed.message === 'string') {
				return parsed;
			}
		} catch {
			// Ignore non-JSON errors.
		}
	}
	return null;
}
