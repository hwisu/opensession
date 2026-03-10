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
	summaryTriggerDetail,
	batchDetail,
	batchStatusDetail,
	lifecycleEnabled,
	lifecycleDetail,
	lifecycleResultDetail,
	lifecycleNextDetail,
	vectorEnabled,
	vectorDetail,
	vectorStatusDetail,
	changeReaderEnabled,
	changeReaderDetail,
	changeReaderQaEnabled,
	changeReaderVoiceEnabled,
	changeReaderVoiceSummary,
	jumpLinks = [],
	onReset,
	onSave,
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
	summaryTriggerDetail: string;
	batchDetail: string;
	batchStatusDetail: string;
	lifecycleEnabled: boolean;
	lifecycleDetail: string;
	lifecycleResultDetail: string;
	lifecycleNextDetail: string;
	vectorEnabled: boolean;
	vectorDetail: string;
	vectorStatusDetail: string;
	changeReaderEnabled: boolean;
	changeReaderDetail: string;
	changeReaderQaEnabled: boolean;
	changeReaderVoiceEnabled: boolean;
	changeReaderVoiceSummary: string;
	jumpLinks?: RuntimeQuickJumpLink[];
	onReset: () => void;
	onSave: () => void;
	onJumpToSection: (sectionId: string) => void;
} = $props();

const isKorean = $derived($appLocale === 'ko');

function localize(en: string, ko: string): string {
	return isKorean ? ko : en;
}

function quickStatusLabel(enabled: boolean): string {
	return enabled ? localize('On', '켜짐') : localize('Off', '꺼짐');
}

function quickStatusClasses(enabled: boolean): string {
	return enabled
		? 'border-emerald-500/60 bg-emerald-500/10 text-emerald-700'
		: 'border-border/80 bg-bg-secondary text-text-secondary';
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
						{localize('Compact runtime summary', '간단한 런타임 요약')}
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
					'This panel only summarizes the current draft. Open the sections on the left when you need to tune details.',
					'이 패널은 현재 초안 상태만 요약합니다. 세부 조정이 필요하면 왼쪽 섹션을 열어 수정하세요.',
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
				{localize('Current Defaults', '현재 기본값')}
			</p>
			<div class="space-y-2 rounded border border-border/60 bg-bg-primary px-3 py-3 text-[11px] text-text-secondary">
				<div class="flex items-center justify-between gap-3">
					<span class="text-text-muted">{localize('Provider', '프로바이더')}</span>
					<code>{provider}</code>
				</div>
				<div class="flex items-center justify-between gap-3">
					<span class="text-text-muted">{localize('Storage', '저장소')}</span>
					<code>{storageBackend}</code>
				</div>
				<div class="flex items-center justify-between gap-3">
					<span class="text-text-muted">{localize('View', '보기')}</span>
					<span>{sessionDefaultView}</span>
				</div>
				<div class="flex items-center justify-between gap-3">
					<span class="text-text-muted">{localize('Transport', '전송')}</span>
					<span>{providerTransport}</span>
				</div>
				<div class="flex items-center justify-between gap-3">
					<span class="text-text-muted">{localize('Batch scope', '배치 범위')}</span>
					<span>{batchScopeLabel}</span>
				</div>
			</div>
		</div>

		<div class="space-y-2">
			<p class="text-[11px] font-semibold uppercase tracking-[0.08em] text-text-muted">
				{localize('Background', '백그라운드')}
			</p>
			<div class="space-y-2 rounded border border-border/60 bg-bg-primary px-3 py-3 text-[11px] text-text-secondary">
				<div>
					<p class="text-xs font-semibold text-text-primary">{localize('Summary on save', '저장 시 요약')}</p>
					<p class="mt-1">{summaryTriggerDetail}</p>
				</div>
				<div class="border-t border-border/60 pt-2">
					<p class="text-xs font-semibold text-text-primary">{localize('Summary batch', '요약 배치')}</p>
					<p class="mt-1">{batchDetail}</p>
					<p class="mt-1 text-text-muted">{batchStatusDetail}</p>
				</div>
				<div class="border-t border-border/60 pt-2">
					<p class="text-xs font-semibold text-text-primary">{localize('Lifecycle cleanup', '수명주기 정리')}</p>
					<p class="mt-1">{lifecycleDetail}</p>
					<p class="mt-1 text-text-muted">{lifecycleResultDetail}</p>
					<p class="mt-1 text-text-muted">{lifecycleNextDetail}</p>
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
						<span
							class={`inline-flex items-center border px-2 py-1 text-[11px] font-semibold ${quickStatusClasses(vectorEnabled)}`}
						>
							{quickStatusLabel(vectorEnabled)}
						</span>
					</div>
				</div>

				<div class="rounded border border-border/60 bg-bg-primary px-3 py-2">
					<div class="flex items-start justify-between gap-3">
						<div class="min-w-0">
							<p class="text-xs font-semibold text-text-primary">{localize('Change reader', '변경 리더')}</p>
							<p class="mt-1 text-[11px] text-text-secondary">{changeReaderDetail}</p>
							<p class="mt-1 text-[11px] text-text-muted">
								{localize(
									`Follow-up questions ${quickStatusLabel(changeReaderQaEnabled).toLowerCase()} · voice ${quickStatusLabel(changeReaderVoiceEnabled).toLowerCase()}`,
									`후속 질문 ${quickStatusLabel(changeReaderQaEnabled)} · 음성 ${quickStatusLabel(changeReaderVoiceEnabled)}`,
								)}
							</p>
						</div>
						<span
							class={`inline-flex items-center border px-2 py-1 text-[11px] font-semibold ${quickStatusClasses(changeReaderEnabled)}`}
						>
							{quickStatusLabel(changeReaderEnabled)}
						</span>
					</div>
				</div>

				<div class="rounded border border-border/60 bg-bg-primary px-3 py-2">
					<div class="flex items-start justify-between gap-3">
						<div class="min-w-0">
							<p class="text-xs font-semibold text-text-primary">{localize('Voice playback', '음성 재생')}</p>
							<p class="mt-1 text-[11px] text-text-secondary">{changeReaderVoiceSummary}</p>
						</div>
						<span
							class={`inline-flex items-center border px-2 py-1 text-[11px] font-semibold ${quickStatusClasses(changeReaderVoiceEnabled)}`}
						>
							{quickStatusLabel(changeReaderVoiceEnabled)}
						</span>
					</div>
				</div>

				<div class="rounded border border-border/60 bg-bg-primary px-3 py-2">
					<div class="flex items-start justify-between gap-3">
						<div class="min-w-0">
							<p class="text-xs font-semibold text-text-primary">{localize('Lifecycle cleanup', '수명주기 정리')}</p>
							<p class="mt-1 text-[11px] text-text-secondary">{lifecycleDetail}</p>
						</div>
						<span
							class={`inline-flex items-center border px-2 py-1 text-[11px] font-semibold ${quickStatusClasses(lifecycleEnabled)}`}
						>
							{quickStatusLabel(lifecycleEnabled)}
						</span>
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
