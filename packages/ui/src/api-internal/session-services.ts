import { Effect } from 'effect';
import { createSessionReadCore } from '../session-read-core';
import type {
	CapabilitiesResponse,
	DesktopChangeQuestionResponse,
	DesktopChangeReaderScope,
	DesktopChangeReadResponse,
	DesktopChangeReaderTtsResponse,
	DesktopHandoffBuildResponse,
	DesktopLifecycleCleanupStatusResponse,
	DesktopQuickShareResponse,
	DesktopRuntimeSettingsResponse,
	DesktopRuntimeSettingsUpdateRequest,
	DesktopSummaryBatchStatusResponse,
	DesktopSessionSummaryResponse,
	DesktopSummaryProviderDetectResponse,
	DesktopVectorIndexStatusResponse,
	DesktopVectorInstallStatusResponse,
	DesktopVectorPreflightResponse,
	DesktopVectorSearchResponse,
	Session,
	SessionDetail,
	SessionListResponse,
	AuthProvidersResponse,
} from '../types';
import type { SessionListParams } from '../session-adapter';
import { normalizeSessionAdapterError } from './errors';
import {
	createRuntimeSessionReadAdapter,
	type RuntimeEnv,
	RuntimeEnvTag,
} from './runtime';

function withSessionReadCore<A>(
	handler: (core: ReturnType<typeof createSessionReadCore>) => Promise<A>,
): Effect.Effect<A, ReturnType<typeof normalizeSessionAdapterError>, RuntimeEnv> {
	return Effect.gen(function* () {
		const runtime = yield* RuntimeEnvTag;
		const core = createSessionReadCore(createRuntimeSessionReadAdapter(runtime));
		return yield* Effect.tryPromise({
			try: () => handler(core),
			catch: normalizeSessionAdapterError,
		});
	});
}

export function listSessionsEffect(
	params?: SessionListParams,
): Effect.Effect<SessionListResponse, ReturnType<typeof normalizeSessionAdapterError>, RuntimeEnv> {
	return withSessionReadCore((core) => core.listSessions(params));
}

export function listSessionReposEffect(): Effect.Effect<
	string[],
	ReturnType<typeof normalizeSessionAdapterError>,
	RuntimeEnv
> {
	return withSessionReadCore((core) => core.listRepos());
}

export function getSessionEffect(
	id: string,
): Effect.Effect<Session, ReturnType<typeof normalizeSessionAdapterError>, RuntimeEnv> {
	return withSessionReadCore((core) => core.getSession(id));
}

export function getSessionDetailEffect(
	id: string,
): Effect.Effect<SessionDetail, ReturnType<typeof normalizeSessionAdapterError>, RuntimeEnv> {
	return withSessionReadCore((core) => core.getSessionDetail(id));
}

export function getSessionSemanticSummaryEffect(
	sessionId: string,
): Effect.Effect<
	DesktopSessionSummaryResponse,
	ReturnType<typeof normalizeSessionAdapterError>,
	RuntimeEnv
> {
	return withSessionReadCore((core) => core.getSessionSummary(sessionId));
}

export function regenerateSessionSemanticSummaryEffect(
	sessionId: string,
): Effect.Effect<
	DesktopSessionSummaryResponse,
	ReturnType<typeof normalizeSessionAdapterError>,
	RuntimeEnv
> {
	return withSessionReadCore((core) => core.regenerateSessionSummary(sessionId));
}

export function buildSessionHandoffEffect(
	sessionId: string,
	pinLatest: boolean,
): Effect.Effect<
	DesktopHandoffBuildResponse,
	ReturnType<typeof normalizeSessionAdapterError>,
	RuntimeEnv
> {
	return withSessionReadCore((core) => core.buildHandoff(sessionId, pinLatest));
}

export function quickShareSessionEffect(
	sessionId: string,
	remote: string | null,
): Effect.Effect<
	DesktopQuickShareResponse,
	ReturnType<typeof normalizeSessionAdapterError>,
	RuntimeEnv
> {
	return withSessionReadCore((core) => core.quickShareSession(sessionId, remote));
}

export function readSessionChangesEffect(
	sessionId: string,
	scope?: DesktopChangeReaderScope | null,
): Effect.Effect<
	DesktopChangeReadResponse,
	ReturnType<typeof normalizeSessionAdapterError>,
	RuntimeEnv
> {
	return withSessionReadCore((core) => core.readSessionChanges(sessionId, scope));
}

export function askSessionChangesEffect(
	sessionId: string,
	question: string,
	scope?: DesktopChangeReaderScope | null,
): Effect.Effect<
	DesktopChangeQuestionResponse,
	ReturnType<typeof normalizeSessionAdapterError>,
	RuntimeEnv
> {
	return withSessionReadCore((core) => core.askSessionChanges(sessionId, question, scope));
}

export function changeReaderTextToSpeechEffect(
	text: string,
	sessionId?: string | null,
	scope?: DesktopChangeReaderScope | null,
): Effect.Effect<
	DesktopChangeReaderTtsResponse,
	ReturnType<typeof normalizeSessionAdapterError>,
	RuntimeEnv
> {
	return withSessionReadCore((core) => core.changeReaderTts(text, sessionId, scope));
}

export function getRuntimeSettingsEffect(): Effect.Effect<
	DesktopRuntimeSettingsResponse,
	ReturnType<typeof normalizeSessionAdapterError>,
	RuntimeEnv
> {
	return withSessionReadCore((core) => core.getRuntimeSettings());
}

export function updateRuntimeSettingsEffect(
	request: DesktopRuntimeSettingsUpdateRequest,
): Effect.Effect<
	DesktopRuntimeSettingsResponse,
	ReturnType<typeof normalizeSessionAdapterError>,
	RuntimeEnv
> {
	return withSessionReadCore((core) => core.updateRuntimeSettings(request));
}

export function getLifecycleCleanupStatusEffect(): Effect.Effect<
	DesktopLifecycleCleanupStatusResponse,
	ReturnType<typeof normalizeSessionAdapterError>,
	RuntimeEnv
> {
	return withSessionReadCore((core) => core.lifecycleCleanupStatus());
}

export function runSummaryBatchEffect(): Effect.Effect<
	DesktopSummaryBatchStatusResponse,
	ReturnType<typeof normalizeSessionAdapterError>,
	RuntimeEnv
> {
	return withSessionReadCore((core) => core.summaryBatchRun());
}

export function getSummaryBatchStatusEffect(): Effect.Effect<
	DesktopSummaryBatchStatusResponse,
	ReturnType<typeof normalizeSessionAdapterError>,
	RuntimeEnv
> {
	return withSessionReadCore((core) => core.summaryBatchStatus());
}

export function detectSummaryProviderEffect(): Effect.Effect<
	DesktopSummaryProviderDetectResponse,
	ReturnType<typeof normalizeSessionAdapterError>,
	RuntimeEnv
> {
	return withSessionReadCore((core) => core.detectSummaryProvider());
}

export function vectorPreflightEffect(): Effect.Effect<
	DesktopVectorPreflightResponse,
	ReturnType<typeof normalizeSessionAdapterError>,
	RuntimeEnv
> {
	return withSessionReadCore((core) => core.vectorPreflight());
}

export function vectorInstallModelEffect(
	model: string,
): Effect.Effect<
	DesktopVectorInstallStatusResponse,
	ReturnType<typeof normalizeSessionAdapterError>,
	RuntimeEnv
> {
	return withSessionReadCore((core) => core.vectorInstallModel(model));
}

export function vectorIndexRebuildEffect(): Effect.Effect<
	DesktopVectorIndexStatusResponse,
	ReturnType<typeof normalizeSessionAdapterError>,
	RuntimeEnv
> {
	return withSessionReadCore((core) => core.vectorIndexRebuild());
}

export function vectorIndexStatusEffect(): Effect.Effect<
	DesktopVectorIndexStatusResponse,
	ReturnType<typeof normalizeSessionAdapterError>,
	RuntimeEnv
> {
	return withSessionReadCore((core) => core.vectorIndexStatus());
}

export function searchSessionsVectorEffect(
	query: string,
	cursor?: string | null,
	limit?: number,
): Effect.Effect<
	DesktopVectorSearchResponse,
	ReturnType<typeof normalizeSessionAdapterError>,
	RuntimeEnv
> {
	return withSessionReadCore((core) => core.searchSessionsVector(query, cursor, limit));
}

export function getAuthProvidersEffect(): Effect.Effect<
	AuthProvidersResponse,
	ReturnType<typeof normalizeSessionAdapterError>,
	RuntimeEnv
> {
	return Effect.gen(function* () {
		const runtime = yield* RuntimeEnvTag;
		const adapter = createRuntimeSessionReadAdapter(runtime);
		return yield* Effect.tryPromise({
			try: () => adapter.getAuthProviders(),
			catch: normalizeSessionAdapterError,
		});
	});
}

export function getApiCapabilitiesEffect(): Effect.Effect<
	CapabilitiesResponse,
	ReturnType<typeof normalizeSessionAdapterError>,
	RuntimeEnv
> {
	return Effect.gen(function* () {
		const runtime = yield* RuntimeEnvTag;
		const adapter = createRuntimeSessionReadAdapter(runtime);
		return yield* Effect.tryPromise({
			try: () => adapter.getCapabilities(),
			catch: normalizeSessionAdapterError,
		});
	});
}
