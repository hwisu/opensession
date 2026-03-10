<script lang="ts">
import { handleAuthCallback } from '../api';
import { appLocale } from '../i18n';

const {
	onSuccess = () => {},
	onError = () => {},
}: {
	onSuccess?: () => void;
	onError?: () => void;
} = $props();

let status = $state<'loading' | 'error'>('loading');
const isKorean = $derived($appLocale === 'ko');

function localize(en: string, ko: string): string {
	return isKorean ? ko : en;
}

$effect(() => {
	handleAuthCallback()
		.then((ok) => {
			if (ok) {
				onSuccess();
				return;
			}
			status = 'error';
			onError();
		})
		.catch(() => {
			status = 'error';
			onError();
		});
});
</script>

<div class="flex items-center justify-center pt-24">
	{#if status === 'loading'}
		<p class="text-xs text-text-muted">{localize('Completing sign in...', '로그인을 마무리하는 중...')}</p>
	{:else}
		<p class="text-xs text-error">
			{localize('Authentication failed. Please try again.', '인증에 실패했습니다. 다시 시도해 주세요.')}
		</p>
	{/if}
</div>
