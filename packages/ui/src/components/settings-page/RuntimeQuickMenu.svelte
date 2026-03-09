<script lang="ts">
import type {
	DesktopSummaryProviderId,
	DesktopSummaryProviderTransport,
	DesktopSummaryStorageBackend,
} from '../../types';
import { appLocale } from '../../i18n';
import type { RuntimeQuickJumpLink } from './models';

const {
	draftDirty,
	runtimeSaving,
	runtimeLoading,
	saveLabel,
	provider,
	storageBackend,
	sessionDefaultView,
	providerTransport,
	batchScopeLabel,
	summaryTriggerAuto,
	summaryTriggerDetail,
	batchAuto,
	batchDetail,
	batchStatusDetail,
	lifecycleEnabled,
	lifecycleDetail,
	lifecycleResultDetail,
	lifecycleNextDetail,
	vectorEnabled,
	vectorToggleDisabled,
	vectorDetail,
	vectorStatusDetail,
	changeReaderEnabled,
	changeReaderDetail,
	changeReaderQaEnabled,
	changeReaderQaDisabled,
	changeReaderVoiceEnabled,
	changeReaderVoiceDisabled,
	changeReaderVoiceBlockedReason,
	changeReaderVoiceSummary,
	jumpLinks = [],
	onReset,
	onSave,
	onProviderChange,
	onStorageBackendChange,
	onToggleSummaryTrigger,
	onToggleBatch,
	onToggleLifecycle,
	onToggleVector,
	onToggleChangeReader,
	onToggleChangeReaderQa,
	onToggleChangeReaderVoice,
	onJumpToSection,
}: {
	draftDirty: boolean;
	runtimeSaving: boolean;
	runtimeLoading: boolean;
	saveLabel: string;
	provider: DesktopSummaryProviderId;
	storageBackend: DesktopSummaryStorageBackend;
	sessionDefaultView: 'full' | 'compressed';
	providerTransport: DesktopSummaryProviderTransport;
	batchScopeLabel: string;
	summaryTriggerAuto: boolean;
	summaryTriggerDetail: string;
	batchAuto: boolean;
	batchDetail: string;
	batchStatusDetail: string;
	lifecycleEnabled: boolean;
	lifecycleDetail: string;
	lifecycleResultDetail: string;
	lifecycleNextDetail: string;
	vectorEnabled: boolean;
	vectorToggleDisabled: boolean;
	vectorDetail: string;
	vectorStatusDetail: string;
	changeReaderEnabled: boolean;
	changeReaderDetail: string;
	changeReaderQaEnabled: boolean;
	changeReaderQaDisabled: boolean;
	changeReaderVoiceEnabled: boolean;
	changeReaderVoiceDisabled: boolean;
	changeReaderVoiceBlockedReason: string | null;
	changeReaderVoiceSummary: string;
	jumpLinks?: RuntimeQuickJumpLink[];
	onReset: () => void;
	onSave: () => void;
	onProviderChange: (provider: DesktopSummaryProviderId) => void;
	onStorageBackendChange: (backend: DesktopSummaryStorageBackend) => void;
	onToggleSummaryTrigger: () => void;
	onToggleBatch: () => void;
	onToggleLifecycle: () => void;
	onToggleVector: () => void;
	onToggleChangeReader: () => void;
	onToggleChangeReaderQa: () => void;
	onToggleChangeReaderVoice: () => void;
	onJumpToSection: (sectionId: string) => void;
} = $props();

const isKorean = $derived($appLocale === 'ko');

function localize(en: string, ko: string): string {
	return isKorean ? ko : en;
}

function quickToggleLabel(enabled: boolean): string {
	return enabled ? localize('On', '켜짐') : localize('Off', '꺼짐');
}

function quickToggleClasses(enabled: boolean): string {
	return enabled
		? 'border-emerald-500/60 bg-emerald-500/10 text-emerald-700'
		: 'border-border/80 bg-bg-secondary text-text-secondary';
}

function handleProviderChange(event: Event) {
	onProviderChange((event.currentTarget as HTMLSelectElement).value as DesktopSummaryProviderId);
}

function handleStorageBackendChange(event: Event) {
	onStorageBackendChange(
		(event.currentTarget as HTMLSelectElement).value as DesktopSummaryStorageBackend,
	);
}
</script>

<aside
	class="order-first xl:order-last xl:sticky xl:top-4 xl:max-h-[calc(100vh-1.5rem)] xl:overflow-y-auto"
	data-testid="runtime-quick-menu"
>
	<div class="space-y-4 border border-border bg-bg-secondary p-3 shadow-[0_14px_40px_rgba(15,23,42,0.12)]">
		<div class="space-y-2">
			<div class="flex items-start justify-between gap-3">
				<div>
					<p class="text-[11px] font-semibold uppercase tracking-[0.08em] text-text-muted">
						{localize('Quick Runtime Menu', '빠른 런타임 메뉴')}
					</p>
					<p class="mt-1 text-sm font-semibold text-text-primary">
						{localize('Live draft overview', '실시간 초안 개요')}
					</p>
				</div>
				<span
					class={`inline-flex items-center border px-2 py-1 text-[11px] font-semibold ${
						draftDirty
							? 'border-accent/40 bg-accent/5 text-accent'
							: 'border-border/70 bg-bg-primary text-text-secondary'
					}`}
					data-testid="runtime-quick-draft-state"
				>
					{draftDirty ? localize('Draft', '초안') : localize('Saved', '저장됨')}
				</span>
			</div>
			<p class="text-[11px] text-text-secondary">
				{localize(
					'Flip common on/off controls here, then save once.',
					'자주 쓰는 켜기/끄기 설정을 여기서 바꾸고 한 번만 저장하세요.',
				)}
			</p>
			<div class="grid grid-cols-2 gap-2">
				<button
					type="button"
					data-testid="runtime-quick-reset"
					onclick={onReset}
					disabled={runtimeSaving || !draftDirty}
					class="inline-flex h-9 items-center justify-center border border-border px-2 text-[11px] font-semibold text-text-secondary hover:text-text-primary disabled:opacity-60"
				>
					{localize('Reset', '초기화')}
				</button>
				<button
					type="button"
					data-testid="runtime-quick-save"
					onclick={onSave}
					disabled={runtimeSaving || runtimeLoading}
					class="inline-flex h-9 items-center justify-center border border-transparent bg-accent px-2 text-[11px] font-semibold text-white hover:bg-accent/85 disabled:opacity-60"
				>
					{runtimeSaving ? localize('Saving...', '저장 중...') : saveLabel}
				</button>
			</div>
		</div>

		<div class="space-y-2">
			<p class="text-[11px] font-semibold uppercase tracking-[0.08em] text-text-muted">
				{localize('Current Modes', '현재 모드')}
			</p>
			<label class="block text-[11px] text-text-secondary">
				<span class="mb-1 block text-text-muted">{localize('Provider', '프로바이더')}</span>
				<select
					value={provider}
					onchange={handleProviderChange}
					data-testid="runtime-quick-provider"
					class="h-9 w-full border border-border bg-bg-primary px-2 text-xs text-text-primary"
				>
					<option value="disabled">{localize('disabled', '사용 안 함')}</option>
					<option value="ollama">ollama</option>
					<option value="codex_exec">codex_exec</option>
					<option value="claude_cli">claude_cli</option>
				</select>
			</label>
			<label class="block text-[11px] text-text-secondary">
				<span class="mb-1 block text-text-muted">{localize('Storage backend', '저장소 백엔드')}</span>
				<select
					value={storageBackend}
					onchange={handleStorageBackendChange}
					data-testid="runtime-quick-storage"
					class="h-9 w-full border border-border bg-bg-primary px-2 text-xs text-text-primary"
				>
					<option value="hidden_ref">hidden_ref</option>
					<option value="local_db">local_db</option>
					<option value="none">{localize('none', '없음')}</option>
				</select>
			</label>
			<div class="rounded border border-border/60 bg-bg-primary px-2 py-2 text-[11px] text-text-secondary">
				<p>{localize('View', '보기')} {sessionDefaultView}</p>
				<p class="mt-1">{localize('Transport', '전달 방식')} {providerTransport}</p>
				<p class="mt-1">{localize('Batch scope', '배치 범위')} {batchScopeLabel}</p>
			</div>
		</div>

		<div class="space-y-2">
			<p class="text-[11px] font-semibold uppercase tracking-[0.08em] text-text-muted">
				{localize('Background / Auto', '백그라운드 / 자동')}
			</p>
			<div class="space-y-2">
				<div class="rounded border border-border/60 bg-bg-primary px-3 py-2">
					<div class="flex items-start justify-between gap-3">
						<div class="min-w-0">
							<p class="text-xs font-semibold text-text-primary">{localize('Summary on save', '저장 시 요약')}</p>
							<p class="mt-1 text-[11px] text-text-secondary">{summaryTriggerDetail}</p>
						</div>
						<button
							type="button"
							data-testid="runtime-quick-toggle-summary-trigger"
							aria-pressed={summaryTriggerAuto}
							onclick={onToggleSummaryTrigger}
							class={`inline-flex min-w-[3.5rem] items-center justify-center border px-2 py-1 text-[11px] font-semibold ${quickToggleClasses(summaryTriggerAuto)}`}
						>
							{quickToggleLabel(summaryTriggerAuto)}
						</button>
					</div>
				</div>

				<div class="rounded border border-border/60 bg-bg-primary px-3 py-2">
					<div class="flex items-start justify-between gap-3">
						<div class="min-w-0">
							<p class="text-xs font-semibold text-text-primary">{localize('Batch on app start', '앱 시작 시 배치 실행')}</p>
							<p class="mt-1 text-[11px] text-text-secondary">{batchDetail}</p>
							<p class="mt-1 text-[11px] text-text-muted">{batchStatusDetail}</p>
						</div>
						<button
							type="button"
							data-testid="runtime-quick-toggle-batch"
							aria-pressed={batchAuto}
							onclick={onToggleBatch}
							class={`inline-flex min-w-[3.5rem] items-center justify-center border px-2 py-1 text-[11px] font-semibold ${quickToggleClasses(batchAuto)}`}
						>
							{quickToggleLabel(batchAuto)}
						</button>
					</div>
				</div>

				<div class="rounded border border-border/60 bg-bg-primary px-3 py-2">
					<div class="flex items-start justify-between gap-3">
						<div class="min-w-0">
							<p class="text-xs font-semibold text-text-primary">{localize('Lifecycle cleanup', '수명주기 정리')}</p>
							<p class="mt-1 text-[11px] text-text-secondary">{lifecycleDetail}</p>
							<p class="mt-1 text-[11px] text-text-muted">{lifecycleResultDetail}</p>
							<p class="mt-1 text-[11px] text-text-muted">{lifecycleNextDetail}</p>
						</div>
						<button
							type="button"
							data-testid="runtime-quick-toggle-lifecycle"
							aria-pressed={lifecycleEnabled}
							onclick={onToggleLifecycle}
							class={`inline-flex min-w-[3.5rem] items-center justify-center border px-2 py-1 text-[11px] font-semibold ${quickToggleClasses(lifecycleEnabled)}`}
						>
							{quickToggleLabel(lifecycleEnabled)}
						</button>
					</div>
				</div>
			</div>
		</div>

		<div class="space-y-2">
			<p class="text-[11px] font-semibold uppercase tracking-[0.08em] text-text-muted">
				{localize('Features', '기능')}
			</p>
			<div class="space-y-2">
				<div class="rounded border border-border/60 bg-bg-primary px-3 py-2">
					<div class="flex items-start justify-between gap-3">
						<div class="min-w-0">
							<p class="text-xs font-semibold text-text-primary">{localize('Vector search', '벡터 검색')}</p>
							<p class="mt-1 text-[11px] text-text-secondary">{vectorDetail}</p>
							<p class="mt-1 text-[11px] text-text-muted">{vectorStatusDetail}</p>
						</div>
						<button
							type="button"
							data-testid="runtime-quick-toggle-vector"
							aria-pressed={vectorEnabled}
							disabled={vectorToggleDisabled}
							onclick={onToggleVector}
							class={`inline-flex min-w-[3.5rem] items-center justify-center border px-2 py-1 text-[11px] font-semibold ${quickToggleClasses(vectorEnabled)} disabled:opacity-60`}
						>
							{quickToggleLabel(vectorEnabled)}
						</button>
					</div>
				</div>

				<div class="rounded border border-border/60 bg-bg-primary px-3 py-2">
					<div class="flex items-start justify-between gap-3">
						<div class="min-w-0">
							<p class="text-xs font-semibold text-text-primary">{localize('Change reader', '변경 리더')}</p>
							<p class="mt-1 text-[11px] text-text-secondary">{changeReaderDetail}</p>
						</div>
						<button
							type="button"
							data-testid="runtime-quick-toggle-change-reader"
							aria-pressed={changeReaderEnabled}
							onclick={onToggleChangeReader}
							class={`inline-flex min-w-[3.5rem] items-center justify-center border px-2 py-1 text-[11px] font-semibold ${quickToggleClasses(changeReaderEnabled)}`}
						>
							{quickToggleLabel(changeReaderEnabled)}
						</button>
					</div>
				</div>

				<div class="rounded border border-border/60 bg-bg-primary px-3 py-2">
					<div class="min-w-0">
						<p class="text-xs font-semibold text-text-primary">{localize('Change reader modes', '변경 리더 모드')}</p>
						<p class="mt-1 text-[11px] text-text-secondary">
							{localize(
								'Optional subfeatures on top of the text reader.',
								'텍스트 리더 위에 추가로 켤 수 있는 부가 기능입니다.',
							)}
						</p>
					</div>
					<div class="mt-2 space-y-2">
						<div class="flex items-start justify-between gap-3 rounded border border-border/60 px-2 py-2">
							<div class="min-w-0">
								<p class="text-xs font-semibold text-text-primary">{localize('Follow-up questions', '후속 질문')}</p>
								<p class="mt-1 text-[11px] text-text-secondary">
									{localize('text questions about the current change context', '현재 변경 맥락에 대해 텍스트로 추가 질문')}
								</p>
							</div>
							<button
								type="button"
								data-testid="runtime-quick-toggle-change-reader-qa"
								aria-pressed={changeReaderQaEnabled}
								disabled={changeReaderQaDisabled}
								onclick={onToggleChangeReaderQa}
								class={`inline-flex min-w-[3.5rem] items-center justify-center border px-2 py-1 text-[11px] font-semibold ${quickToggleClasses(changeReaderQaEnabled)} disabled:opacity-60`}
							>
								{quickToggleLabel(changeReaderQaEnabled)}
							</button>
						</div>

						<div class="flex items-start justify-between gap-3 rounded border border-border/60 px-2 py-2">
							<div class="min-w-0">
								<p class="text-xs font-semibold text-text-primary">{localize('Voice playback', '음성 재생')}</p>
								<p class="mt-1 text-[11px] text-text-secondary">{changeReaderVoiceSummary}</p>
							</div>
							<button
								type="button"
								data-testid="runtime-quick-toggle-change-reader-voice"
								aria-pressed={changeReaderVoiceEnabled}
								disabled={changeReaderVoiceDisabled}
								title={changeReaderVoiceBlockedReason ?? undefined}
								onclick={onToggleChangeReaderVoice}
								class={`inline-flex min-w-[3.5rem] items-center justify-center border px-2 py-1 text-[11px] font-semibold ${quickToggleClasses(changeReaderVoiceEnabled)} disabled:opacity-60`}
							>
								{quickToggleLabel(changeReaderVoiceEnabled)}
							</button>
						</div>
					</div>
				</div>
			</div>
		</div>

		<div class="space-y-2">
			<p class="text-[11px] font-semibold uppercase tracking-[0.08em] text-text-muted">
				{localize('Jump To Section', '섹션 바로가기')}
			</p>
			<div class="flex flex-wrap gap-2">
				{#each jumpLinks as item}
					<button
						type="button"
						onclick={() => onJumpToSection(item.id)}
						class="inline-flex h-8 items-center border border-border px-2 text-[11px] font-semibold text-text-secondary hover:text-text-primary"
					>
						{item.label}
					</button>
				{/each}
			</div>
		</div>
	</div>
</aside>
