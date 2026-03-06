import type { ParsePreviewErrorResponse } from '../types';
import { SessionReadCoreError } from '../session-read-core';

function parseBodyErrorShape(body: string): {
	code?: string;
	message?: string;
	details?: Record<string, unknown> | null;
} | null {
	try {
		return JSON.parse(body) as {
			code?: string;
			message?: string;
			details?: Record<string, unknown> | null;
		};
	} catch {
		return null;
	}
}

export class ApiError extends Error {
	constructor(
		public status: number,
		public body: string,
		public code: string = 'unknown',
		public details: Record<string, unknown> | null = null,
	) {
		let message =
			body.trimStart().startsWith('<') ? `Server returned ${status}` : body.slice(0, 200);
		let resolvedCode = code;
		let resolvedDetails = details;
		if (!body.trimStart().startsWith('<')) {
			const parsed = parseBodyErrorShape(body);
			if (parsed) {
				if (typeof parsed.message === 'string' && parsed.message.trim()) {
					message = parsed.message.trim();
				}
				if (typeof parsed.code === 'string' && parsed.code.trim()) {
					resolvedCode = parsed.code.trim();
				}
				if (parsed.details && typeof parsed.details === 'object') {
					resolvedDetails = parsed.details;
				}
			}
		}
		super(message);
		this.code = resolvedCode;
		this.details = resolvedDetails;
	}
}

export class PreviewApiError extends Error {
	constructor(
		public status: number,
		public payload: ParsePreviewErrorResponse,
	) {
		super(payload.message);
	}
}

export function normalizeSessionAdapterError(error: unknown): ApiError {
	if (error instanceof ApiError) return error;
	if (error instanceof SessionReadCoreError) {
		return new ApiError(error.status, error.body, error.code, error.details);
	}
	return new ApiError(500, '{"message":"Session adapter request failed"}');
}
