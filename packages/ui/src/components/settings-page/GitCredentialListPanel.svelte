<script lang="ts">
import { appLocale } from '../../i18n';
import type { GitCredentialSummary } from '../../types';

const {
	credentials,
	credentialsLoading,
	deletingCredentialId,
	onDeleteCredential,
}: {
	credentials: GitCredentialSummary[];
	credentialsLoading: boolean;
	deletingCredentialId: string | null;
	onDeleteCredential: (id: string) => void;
} = $props();

const isKorean = $derived($appLocale === 'ko');
</script>

<div class="mt-4 border border-border/70">
	<div class="grid grid-cols-[1.1fr_1fr_1fr_auto] gap-2 border-b border-border bg-bg-primary px-3 py-2 text-[11px] uppercase tracking-[0.08em] text-text-muted">
		<span>{isKorean ? '라벨' : 'Label'}</span>
		<span>{isKorean ? '호스트' : 'Host'}</span>
		<span>{isKorean ? '경로 접두사' : 'Path Prefix'}</span>
		<span>{isKorean ? '동작' : 'Action'}</span>
	</div>
	{#if credentialsLoading}
		<div class="px-3 py-3 text-xs text-text-muted">{isKorean ? '자격 증명을 불러오는 중...' : 'Loading credentials...'}</div>
	{:else if credentials.length === 0}
		<div class="px-3 py-3 text-xs text-text-muted">
			{isKorean ? '등록된 수동 자격 증명이 없습니다.' : 'No manual credentials registered.'}
		</div>
	{:else}
		{#each credentials as credential}
			<div class="grid grid-cols-[1.1fr_1fr_1fr_auto] items-center gap-2 border-b border-border/60 px-3 py-2 text-xs">
				<div class="text-text-primary">{credential.label}</div>
				<div class="font-mono text-[11px] text-text-secondary">{credential.host}</div>
				<div class="font-mono text-[11px] text-text-secondary">{credential.path_prefix || '*'}</div>
				<button
					type="button"
					data-testid={'git-credential-delete-' + credential.id}
					disabled={deletingCredentialId === credential.id}
					onclick={() => onDeleteCredential(credential.id)}
					class="border border-border px-2 py-1 text-[11px] text-text-secondary hover:text-text-primary disabled:opacity-60"
				>
					{deletingCredentialId === credential.id ? (isKorean ? '삭제 중...' : 'Deleting...') : (isKorean ? '삭제' : 'Delete')}
				</button>
			</div>
		{/each}
	{/if}
</div>
