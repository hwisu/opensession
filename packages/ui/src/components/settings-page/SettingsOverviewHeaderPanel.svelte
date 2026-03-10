<script lang="ts">
	import { appLocale } from '../../i18n';

	const isKorean = $derived($appLocale === 'ko');
</script>

<header
	id="settings-section-overview"
	class="scroll-mt-24 border border-border bg-bg-secondary px-4 py-3"
>
	<p class="text-[11px] uppercase tracking-[0.12em] text-text-muted">{isKorean ? '계정' : 'Account'}</p>
	<h1 class="mt-1 text-3xl font-semibold tracking-tight text-text-primary">{isKorean ? '설정' : 'Settings'}</h1>
	<p class="mt-1 text-sm text-text-secondary">
		{isKorean ? '개인 프로필과 API 접근 제어입니다.' : 'Personal profile and API access controls.'}
	</p>
</header>
