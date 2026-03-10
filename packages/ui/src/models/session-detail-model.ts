import { ApiError } from '../api-internal/errors';
import type {
	DesktopChangeQuestionResponse,
	DesktopChangeReaderScope,
	DesktopChangeReadResponse,
	DesktopSessionSummaryResponse,
	DesktopRuntimeSettingsResponse,
	Session,
	SessionDetail,
} from '../types';

export interface SessionDetailLoadResult {
	session: Session | null;
	detail: SessionDetail | null;
	error: string | null;
	errorCode: string | null;
	semanticSummary: DesktopSessionSummaryResponse | null;
	summaryError: string | null;
	changeReaderSupported: boolean;
	changeReaderEnabled: boolean;
	changeReaderQaEnabled: boolean;
	changeReaderScope: DesktopChangeReaderScope;
	changeReaderVoiceEnabled: boolean;
	changeReaderVoiceConfigured: boolean;
	changeReaderRuntimeError: string | null;
}

export interface SessionDetailModelDeps {
	getSession: (id: string) => Promise<Session>;
	getSessionDetail: (id: string) => Promise<SessionDetail>;
	getSessionSemanticSummary: (id: string) => Promise<DesktopSessionSummaryResponse>;
	getRuntimeSettings: () => Promise<DesktopRuntimeSettingsResponse>;
	regenerateSessionSemanticSummary: (id: string) => Promise<DesktopSessionSummaryResponse>;
	readSessionChanges: (
		id: string,
		scope?: DesktopChangeReaderScope | null,
	) => Promise<DesktopChangeReadResponse>;
	askSessionChanges: (
		id: string,
		question: string,
		scope?: DesktopChangeReaderScope | null,
	) => Promise<DesktopChangeQuestionResponse>;
}

export async function loadSessionDetailState(
	deps: SessionDetailModelDeps,
	sessionId: string,
): Promise<SessionDetailLoadResult> {
	const [sessionResult, summaryResult, runtimeResult] = await Promise.allSettled([
		Promise.all([deps.getSession(sessionId), deps.getSessionDetail(sessionId)]),
		deps.getSessionSemanticSummary(sessionId),
		deps.getRuntimeSettings(),
	]);

	let session: Session | null = null;
	let detail: SessionDetail | null = null;
	let error: string | null = null;
	let errorCode: string | null = null;
	if (sessionResult.status === 'fulfilled') {
		[session, detail] = sessionResult.value;
	} else {
		error =
			sessionResult.reason instanceof Error
				? sessionResult.reason.message
				: 'Failed to load session';
		errorCode = sessionResult.reason instanceof ApiError ? sessionResult.reason.code : null;
	}

	let semanticSummary: DesktopSessionSummaryResponse | null = null;
	let summaryError: string | null = null;
	if (summaryResult.status === 'fulfilled') {
		semanticSummary = summaryResult.value;
	} else {
		summaryError =
			summaryResult.reason instanceof Error
				? summaryResult.reason.message
				: 'Failed to load semantic summary';
	}

	let changeReaderSupported = false;
	let changeReaderEnabled = false;
	let changeReaderQaEnabled = false;
	let changeReaderScope: DesktopChangeReaderScope = 'summary_only';
	let changeReaderVoiceEnabled = false;
	let changeReaderVoiceConfigured = false;
	let changeReaderRuntimeError: string | null = null;
	if (runtimeResult.status === 'fulfilled') {
		const runtime = runtimeResult.value;
		changeReaderSupported = true;
		changeReaderEnabled = runtime.change_reader?.enabled ?? false;
		changeReaderQaEnabled = runtime.change_reader?.qa_enabled ?? false;
		changeReaderScope = runtime.change_reader?.scope ?? 'summary_only';
		changeReaderVoiceEnabled = runtime.change_reader?.voice?.enabled ?? false;
		changeReaderVoiceConfigured = runtime.change_reader?.voice?.api_key_configured ?? false;
	} else {
		changeReaderRuntimeError =
			runtimeResult.reason instanceof ApiError && runtimeResult.reason.status === 501
				? null
				: runtimeResult.reason instanceof Error
					? runtimeResult.reason.message
					: 'Failed to load change reader settings';
	}

	return {
		session,
		detail,
		error,
		errorCode,
		semanticSummary,
		summaryError,
		changeReaderSupported,
		changeReaderEnabled,
		changeReaderQaEnabled,
		changeReaderScope,
		changeReaderVoiceEnabled,
		changeReaderVoiceConfigured,
		changeReaderRuntimeError,
	};
}

export async function regenerateSessionSummary(
	deps: SessionDetailModelDeps,
	sessionId: string,
): Promise<{ semanticSummary: DesktopSessionSummaryResponse | null; summaryError: string | null }> {
	try {
		return {
			semanticSummary: await deps.regenerateSessionSemanticSummary(sessionId),
			summaryError: null,
		};
	} catch (error) {
		return {
			semanticSummary: null,
			summaryError: error instanceof Error ? error.message : 'Failed to regenerate summary',
		};
	}
}

export async function readSessionChangesSurface(
	deps: SessionDetailModelDeps,
	sessionId: string,
	scope: DesktopChangeReaderScope,
): Promise<{
	narrative: string | null;
	citations: string[];
	warning: string | null;
	error: string | null;
}> {
	try {
		const payload = await deps.readSessionChanges(sessionId, scope);
		return {
			narrative: payload.narrative ?? null,
			citations: payload.citations ?? [],
			warning: payload.warning ?? null,
			error: null,
		};
	} catch (error) {
		return {
			narrative: null,
			citations: [],
			warning: null,
			error: error instanceof Error ? error.message : 'Failed to read session changes',
		};
	}
}

export async function askSessionChangesSurface(
	deps: SessionDetailModelDeps,
	sessionId: string,
	question: string,
	scope: DesktopChangeReaderScope,
): Promise<{
	answer: string | null;
	citations: string[];
	warning: string | null;
	error: string | null;
}> {
	const normalizedQuestion = question.trim();
	if (!normalizedQuestion) {
		return {
			answer: null,
			citations: [],
			warning: null,
			error: 'Ask a question first.',
		};
	}
	try {
		const payload = await deps.askSessionChanges(sessionId, normalizedQuestion, scope);
		return {
			answer: payload.answer ?? null,
			citations: payload.citations ?? [],
			warning: payload.warning ?? null,
			error: null,
		};
	} catch (error) {
		return {
			answer: null,
			citations: [],
			warning: null,
			error: error instanceof Error ? error.message : 'Failed to answer question',
		};
	}
}
