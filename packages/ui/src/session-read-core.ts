import { parseHailInput } from './hail-parse';
import type {
	DesktopHandoffBuildResponse,
	DesktopRuntimeSettingsResponse,
	DesktopRuntimeSettingsUpdateRequest,
	DesktopVectorIndexStatusResponse,
	DesktopVectorInstallStatusResponse,
	DesktopVectorPreflightResponse,
	DesktopVectorSearchResponse,
	DesktopSessionSummaryResponse,
	DesktopSummaryProviderDetectResponse,
	Session,
	SessionDetail,
	SessionListResponse,
} from './types';
import {
	SessionAdapterError,
	type SessionListParams,
	type SessionReadAdapter,
} from './session-adapter';

function normalizeMessageFromBody(status: number, body: string): string {
	let msg = body.trimStart().startsWith('<') ? `Server returned ${status}` : body.slice(0, 200);
	if (!body.trimStart().startsWith('<')) {
		try {
			const parsed = JSON.parse(body) as { message?: unknown };
			if (typeof parsed.message === 'string' && parsed.message.trim()) {
				msg = parsed.message.trim();
			}
		} catch {
			// ignore malformed json payloads
		}
	}
	return msg;
}

export class SessionReadCoreError extends Error {
	constructor(
		public code: string,
		public status: number,
		public body: string,
		public details: Record<string, unknown> | null = null,
	) {
		super(normalizeMessageFromBody(status, body));
	}

	static fromUnknown(error: unknown): SessionReadCoreError {
		if (error instanceof SessionReadCoreError) return error;
		if (error instanceof SessionAdapterError) {
			return new SessionReadCoreError(
				error.code,
				error.status,
				error.body,
				error.details as Record<string, unknown> | null,
			);
		}
		return new SessionReadCoreError(
			'session_adapter_request_failed',
			500,
			'{"code":"session_adapter_request_failed","message":"Session adapter request failed","details":null}',
		);
	}
}

export interface SessionReadCore {
	listSessions(params?: SessionListParams): Promise<SessionListResponse>;
	listRepos(): Promise<string[]>;
	getSession(id: string): Promise<Session>;
	getSessionDetail(id: string): Promise<SessionDetail>;
	getSessionSummary(id: string): Promise<DesktopSessionSummaryResponse>;
	regenerateSessionSummary(id: string): Promise<DesktopSessionSummaryResponse>;
	buildHandoff(sessionId: string, pinLatest?: boolean): Promise<DesktopHandoffBuildResponse>;
	getRuntimeSettings(): Promise<DesktopRuntimeSettingsResponse>;
	updateRuntimeSettings(
		request: DesktopRuntimeSettingsUpdateRequest,
	): Promise<DesktopRuntimeSettingsResponse>;
	detectSummaryProvider(): Promise<DesktopSummaryProviderDetectResponse>;
	vectorPreflight(): Promise<DesktopVectorPreflightResponse>;
	vectorInstallModel(model: string): Promise<DesktopVectorInstallStatusResponse>;
	vectorIndexRebuild(): Promise<DesktopVectorIndexStatusResponse>;
	vectorIndexStatus(): Promise<DesktopVectorIndexStatusResponse>;
	searchSessionsVector(
		query: string,
		cursor?: string | null,
		limit?: number,
	): Promise<DesktopVectorSearchResponse>;
	getContractVersion(): Promise<string>;
}

export function createSessionReadCore(adapter: SessionReadAdapter): SessionReadCore {
	return {
		async listSessions(params?: SessionListParams): Promise<SessionListResponse> {
			try {
				return await adapter.listSessions(params);
			} catch (error) {
				throw SessionReadCoreError.fromUnknown(error);
			}
		},
		async listRepos(): Promise<string[]> {
			try {
				return await adapter.listRepos();
			} catch (error) {
				throw SessionReadCoreError.fromUnknown(error);
			}
		},
		async getSession(id: string): Promise<Session> {
			let raw: string;
			try {
				raw = await adapter.getSessionRaw(id);
			} catch (error) {
				throw SessionReadCoreError.fromUnknown(error);
			}

			try {
				return parseHailInput(raw);
			} catch (error) {
				const detail = error instanceof Error ? error.message : String(error);
				throw new SessionReadCoreError(
					'session_payload_parse_failed',
					422,
					JSON.stringify({
						code: 'session_payload_parse_failed',
						message: `Failed to parse session payload: ${detail}`,
					}),
				);
			}
		},
		async getSessionDetail(id: string): Promise<SessionDetail> {
			try {
				return await adapter.getSessionDetail(id);
			} catch (error) {
				throw SessionReadCoreError.fromUnknown(error);
			}
		},
		async getSessionSummary(id: string): Promise<DesktopSessionSummaryResponse> {
			try {
				return await adapter.getSessionSummary(id);
			} catch (error) {
				throw SessionReadCoreError.fromUnknown(error);
			}
		},
		async regenerateSessionSummary(id: string): Promise<DesktopSessionSummaryResponse> {
			try {
				return await adapter.regenerateSessionSummary(id);
			} catch (error) {
				throw SessionReadCoreError.fromUnknown(error);
			}
		},
		async buildHandoff(
			sessionId: string,
			pinLatest: boolean = true,
		): Promise<DesktopHandoffBuildResponse> {
			try {
				return await adapter.buildHandoff(sessionId, pinLatest);
			} catch (error) {
				throw SessionReadCoreError.fromUnknown(error);
			}
		},
		async getRuntimeSettings(): Promise<DesktopRuntimeSettingsResponse> {
			try {
				return await adapter.getRuntimeSettings();
			} catch (error) {
				throw SessionReadCoreError.fromUnknown(error);
			}
		},
		async updateRuntimeSettings(
			request: DesktopRuntimeSettingsUpdateRequest,
		): Promise<DesktopRuntimeSettingsResponse> {
			try {
				return await adapter.updateRuntimeSettings(request);
			} catch (error) {
				throw SessionReadCoreError.fromUnknown(error);
			}
		},
		async detectSummaryProvider(): Promise<DesktopSummaryProviderDetectResponse> {
			try {
				return await adapter.detectSummaryProvider();
			} catch (error) {
				throw SessionReadCoreError.fromUnknown(error);
			}
		},
		async vectorPreflight(): Promise<DesktopVectorPreflightResponse> {
			try {
				return await adapter.vectorPreflight();
			} catch (error) {
				throw SessionReadCoreError.fromUnknown(error);
			}
		},
		async vectorInstallModel(model: string): Promise<DesktopVectorInstallStatusResponse> {
			try {
				return await adapter.vectorInstallModel(model);
			} catch (error) {
				throw SessionReadCoreError.fromUnknown(error);
			}
		},
		async vectorIndexRebuild(): Promise<DesktopVectorIndexStatusResponse> {
			try {
				return await adapter.vectorIndexRebuild();
			} catch (error) {
				throw SessionReadCoreError.fromUnknown(error);
			}
		},
		async vectorIndexStatus(): Promise<DesktopVectorIndexStatusResponse> {
			try {
				return await adapter.vectorIndexStatus();
			} catch (error) {
				throw SessionReadCoreError.fromUnknown(error);
			}
		},
		async searchSessionsVector(
			query: string,
			cursor?: string | null,
			limit?: number,
		): Promise<DesktopVectorSearchResponse> {
			try {
				return await adapter.searchSessionsVector(query, cursor, limit);
			} catch (error) {
				throw SessionReadCoreError.fromUnknown(error);
			}
		},
		async getContractVersion(): Promise<string> {
			try {
				return await adapter.getContractVersion();
			} catch (error) {
				throw SessionReadCoreError.fromUnknown(error);
			}
		},
	};
}
