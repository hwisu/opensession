<script lang="ts">
import { appLocale } from '../../i18n';
const {
	issuing,
	issuedApiKey,
	copyMessage,
	onIssueApiKey,
	onCopyApiKey,
}: {
	issuing: boolean;
	issuedApiKey: string | null;
	copyMessage: string | null;
	onIssueApiKey: () => void;
	onCopyApiKey: () => void;
} = $props();

const isKorean = $derived($appLocale === 'ko');
</script>

<section
	id="settings-section-api-key"
	class="scroll-mt-24 border border-border bg-bg-secondary p-4 xl:max-w-3xl"
>
	<div class="flex flex-wrap items-center justify-between gap-3">
		<div>
			<h2 class="text-sm font-semibold text-text-primary">{isKorean ? '개인 API 키' : 'Personal API Key'}</h2>
			<p class="mt-1 text-xs text-text-secondary">
				{isKorean
					? 'CLI와 자동화 접근용 새 키를 발급합니다. 기존 활성 키는 유예 모드로 전환됩니다.'
					: 'Issue a new key for CLI and automation access. Existing active key moves to grace mode.'}
			</p>
		</div>
		<button
			type="button"
			data-testid="issue-api-key-button"
			onclick={onIssueApiKey}
			disabled={issuing}
			class="bg-accent px-3 py-2 text-xs font-semibold text-white hover:bg-accent/85 disabled:opacity-60"
		>
			{issuing ? (isKorean ? '발급 중...' : 'Issuing...') : issuedApiKey ? (isKorean ? '키 다시 발급' : 'Regenerate key') : (isKorean ? '키 발급' : 'Issue key')}
		</button>
	</div>

	{#if issuedApiKey}
		<div class="mt-4 border border-border/80 bg-bg-primary p-3">
			<p class="mb-2 text-xs text-text-muted">
				{isKorean ? '한 번만 표시됩니다. 지금 이 키를 저장하세요.' : 'Shown once. Save this key now.'}
			</p>
			<code data-testid="issued-api-key" class="block break-all font-mono text-xs text-text-primary">
				{issuedApiKey}
			</code>
			<div class="mt-3 flex items-center gap-2">
				<button
					type="button"
					data-testid="copy-api-key"
					onclick={onCopyApiKey}
					class="border border-border px-2 py-1 text-xs text-text-secondary hover:text-text-primary"
				>
					{isKorean ? '복사' : 'Copy'}
				</button>
				{#if copyMessage}
					<span class="text-xs text-text-muted">{copyMessage}</span>
				{/if}
			</div>
		</div>
	{/if}
</section>
