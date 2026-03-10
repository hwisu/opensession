<script lang="ts">
import { appLocale } from '../../i18n';
import type { UserSettings } from '../../types';

const {
	settings,
	formatDate,
}: {
	settings: UserSettings | null;
	formatDate: (value: string | null | undefined) => string;
} = $props();

const isKorean = $derived($appLocale === 'ko');
</script>

<section
	id="settings-section-profile"
	class="scroll-mt-24 border border-border bg-bg-secondary p-4 xl:max-w-3xl"
>
	<h2 class="text-sm font-semibold text-text-primary">{isKorean ? '프로필' : 'Profile'}</h2>
	{#if settings}
		<dl class="mt-3 grid gap-2 text-xs text-text-secondary sm:grid-cols-[10rem_1fr]">
			<dt>{isKorean ? '사용자 ID' : 'User ID'}</dt>
			<dd class="font-mono text-text-primary">{settings.user_id}</dd>
			<dt>{isKorean ? '닉네임' : 'Nickname'}</dt>
			<dd class="text-text-primary">{settings.nickname}</dd>
			<dt>{isKorean ? '이메일' : 'Email'}</dt>
			<dd class="text-text-primary">{settings.email ?? (isKorean ? '연결되지 않음' : 'not linked')}</dd>
			<dt>{isKorean ? '가입일' : 'Joined'}</dt>
			<dd class="text-text-primary">{formatDate(settings.created_at)}</dd>
			<dt>{isKorean ? '연결된 OAuth' : 'Linked OAuth'}</dt>
			<dd class="text-text-primary">
				{#if settings.oauth_providers.length === 0}
					{isKorean ? '없음' : 'none'}
				{:else}
					{settings.oauth_providers.map((provider) => provider.display_name).join(', ')}
				{/if}
			</dd>
		</dl>
	{:else}
		<p class="mt-2 text-xs text-text-muted">
			{isKorean ? '표시할 프로필 데이터가 없습니다.' : 'No profile data available.'}
		</p>
	{/if}
</section>
