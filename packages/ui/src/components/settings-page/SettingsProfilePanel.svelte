<script lang="ts">
import type { UserSettings } from '../../types';

const {
	settings,
	formatDate,
}: {
	settings: UserSettings | null;
	formatDate: (value: string | null | undefined) => string;
} = $props();
</script>

<section
	id="settings-section-profile"
	class="scroll-mt-24 border border-border bg-bg-secondary p-4 xl:max-w-3xl"
>
	<h2 class="text-sm font-semibold text-text-primary">Profile</h2>
	{#if settings}
		<dl class="mt-3 grid gap-2 text-xs text-text-secondary sm:grid-cols-[10rem_1fr]">
			<dt>User ID</dt>
			<dd class="font-mono text-text-primary">{settings.user_id}</dd>
			<dt>Nickname</dt>
			<dd class="text-text-primary">{settings.nickname}</dd>
			<dt>Email</dt>
			<dd class="text-text-primary">{settings.email ?? 'not linked'}</dd>
			<dt>Joined</dt>
			<dd class="text-text-primary">{formatDate(settings.created_at)}</dd>
			<dt>Linked OAuth</dt>
			<dd class="text-text-primary">
				{#if settings.oauth_providers.length === 0}
					none
				{:else}
					{settings.oauth_providers.map((provider) => provider.display_name).join(', ')}
				{/if}
			</dd>
		</dl>
	{:else}
		<p class="mt-2 text-xs text-text-muted">No profile data available.</p>
	{/if}
</section>
