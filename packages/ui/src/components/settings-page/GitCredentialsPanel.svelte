<script lang="ts">
import { appLocale } from '../../i18n';
import type { GitCredentialSummary } from '../../types';
import GitCredentialEditorPanel from './GitCredentialEditorPanel.svelte';
import GitCredentialListPanel from './GitCredentialListPanel.svelte';

type Props = {
	credentialsSupported: boolean;
	credentials: GitCredentialSummary[];
	credentialsLoading: boolean;
	deletingCredentialId: string | null;
	creatingCredential: boolean;
	credentialLabel: string;
	credentialHost: string;
	credentialPathPrefix: string;
	credentialHeaderName: string;
	credentialHeaderValue: string;
	onSaveCredential: () => void;
	onDeleteCredential: (id: string) => void;
};

let {
	credentialsSupported,
	credentials,
	credentialsLoading,
	deletingCredentialId,
	creatingCredential,
	credentialLabel = $bindable(),
	credentialHost = $bindable(),
	credentialPathPrefix = $bindable(),
	credentialHeaderName = $bindable(),
	credentialHeaderValue = $bindable(),
	onSaveCredential,
	onDeleteCredential,
}: Props = $props();

const isKorean = $derived($appLocale === 'ko');
</script>

<section
	id="settings-section-git-credentials"
	class="scroll-mt-24 border border-border bg-bg-secondary p-4 xl:max-w-3xl"
	data-testid="git-credential-settings"
>
	<div class="space-y-1">
		<h2 class="text-sm font-semibold text-text-primary">{isKorean ? '비공개 Git 자격 증명' : 'Private Git Credentials'}</h2>
		<p class="text-xs text-text-secondary">
			{isKorean
				? '권장: GitHub/GitLab OAuth를 연결하세요. 수동 자격 증명은 비공개 self-managed 또는 generic git remote에 사용됩니다.'
				: 'Preferred: connect GitHub/GitLab OAuth. Manual credentials are used for private self-managed or generic git remotes.'}
		</p>
	</div>

	{#if !credentialsSupported}
		<p class="mt-3 text-xs text-text-muted">
			{isKorean
				? '이 배포 환경은 자격 증명 관리 엔드포인트를 노출하지 않습니다.'
				: 'This deployment does not expose credential management endpoints.'}
		</p>
	{:else}
		<GitCredentialEditorPanel
			bind:credentialLabel
			bind:credentialHost
			bind:credentialPathPrefix
			bind:credentialHeaderName
			bind:credentialHeaderValue
			creatingCredential={creatingCredential}
			onSaveCredential={onSaveCredential}
		/>

		<GitCredentialListPanel
			credentials={credentials}
			credentialsLoading={credentialsLoading}
			deletingCredentialId={deletingCredentialId}
			onDeleteCredential={onDeleteCredential}
		/>

		<p class="mt-2 text-[11px] text-text-muted">
			{isKorean
				? '시크릿 값은 저장 후 다시 표시되지 않습니다. 저장된 값은 at-rest 암호화됩니다.'
				: 'Secrets are never shown again after save. Stored values are encrypted at rest.'}
		</p>
	{/if}
</section>
