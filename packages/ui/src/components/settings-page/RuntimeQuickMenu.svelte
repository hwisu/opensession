<script lang="ts">
import type {
	DesktopSummaryProviderId,
	DesktopSummaryProviderTransport,
	DesktopSummaryStorageBackend,
} from '../../types';
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

function quickToggleLabel(enabled: boolean): string {
	return enabled ? 'On' : 'Off';
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
						Quick Runtime Menu
					</p>
					<p class="mt-1 text-sm font-semibold text-text-primary">Live draft overview</p>
				</div>
				<span
					class={`inline-flex items-center border px-2 py-1 text-[11px] font-semibold ${
						draftDirty
							? 'border-accent/40 bg-accent/5 text-accent'
							: 'border-border/70 bg-bg-primary text-text-secondary'
					}`}
					data-testid="runtime-quick-draft-state"
				>
					{draftDirty ? 'Draft' : 'Saved'}
				</span>
			</div>
			<p class="text-[11px] text-text-secondary">
				Flip common on/off controls here, then save once.
			</p>
			<div class="grid grid-cols-2 gap-2">
				<button
					type="button"
					data-testid="runtime-quick-reset"
					onclick={onReset}
					disabled={runtimeSaving || !draftDirty}
					class="inline-flex h-9 items-center justify-center border border-border px-2 text-[11px] font-semibold text-text-secondary hover:text-text-primary disabled:opacity-60"
				>
					Reset
				</button>
				<button
					type="button"
					data-testid="runtime-quick-save"
					onclick={onSave}
					disabled={runtimeSaving || runtimeLoading}
					class="inline-flex h-9 items-center justify-center border border-transparent bg-accent px-2 text-[11px] font-semibold text-white hover:bg-accent/85 disabled:opacity-60"
				>
					{runtimeSaving ? 'Saving...' : saveLabel}
				</button>
			</div>
		</div>

		<div class="space-y-2">
			<p class="text-[11px] font-semibold uppercase tracking-[0.08em] text-text-muted">
				Current Modes
			</p>
			<label class="block text-[11px] text-text-secondary">
				<span class="mb-1 block text-text-muted">Provider</span>
				<select
					value={provider}
					onchange={handleProviderChange}
					data-testid="runtime-quick-provider"
					class="h-9 w-full border border-border bg-bg-primary px-2 text-xs text-text-primary"
				>
					<option value="disabled">disabled</option>
					<option value="ollama">ollama</option>
					<option value="codex_exec">codex_exec</option>
					<option value="claude_cli">claude_cli</option>
				</select>
			</label>
			<label class="block text-[11px] text-text-secondary">
				<span class="mb-1 block text-text-muted">Storage backend</span>
				<select
					value={storageBackend}
					onchange={handleStorageBackendChange}
					data-testid="runtime-quick-storage"
					class="h-9 w-full border border-border bg-bg-primary px-2 text-xs text-text-primary"
				>
					<option value="hidden_ref">hidden_ref</option>
					<option value="local_db">local_db</option>
					<option value="none">none</option>
				</select>
			</label>
			<div class="rounded border border-border/60 bg-bg-primary px-2 py-2 text-[11px] text-text-secondary">
				<p>View {sessionDefaultView}</p>
				<p class="mt-1">Transport {providerTransport}</p>
				<p class="mt-1">Batch scope {batchScopeLabel}</p>
			</div>
		</div>

		<div class="space-y-2">
			<p class="text-[11px] font-semibold uppercase tracking-[0.08em] text-text-muted">
				Background / Auto
			</p>
			<div class="space-y-2">
				<div class="rounded border border-border/60 bg-bg-primary px-3 py-2">
					<div class="flex items-start justify-between gap-3">
						<div class="min-w-0">
							<p class="text-xs font-semibold text-text-primary">Summary on save</p>
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
							<p class="text-xs font-semibold text-text-primary">Batch on app start</p>
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
							<p class="text-xs font-semibold text-text-primary">Lifecycle cleanup</p>
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
				Features
			</p>
			<div class="space-y-2">
				<div class="rounded border border-border/60 bg-bg-primary px-3 py-2">
					<div class="flex items-start justify-between gap-3">
						<div class="min-w-0">
							<p class="text-xs font-semibold text-text-primary">Vector search</p>
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
							<p class="text-xs font-semibold text-text-primary">Change reader</p>
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
						<p class="text-xs font-semibold text-text-primary">Change reader modes</p>
						<p class="mt-1 text-[11px] text-text-secondary">
							Optional subfeatures on top of the text reader.
						</p>
					</div>
					<div class="mt-2 space-y-2">
						<div class="flex items-start justify-between gap-3 rounded border border-border/60 px-2 py-2">
							<div class="min-w-0">
								<p class="text-xs font-semibold text-text-primary">Follow-up Q&amp;A</p>
								<p class="mt-1 text-[11px] text-text-secondary">
									text questions about the current change context
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
								<p class="text-xs font-semibold text-text-primary">Voice playback</p>
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
				Jump To Section
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
