<script lang="ts">
	import { goto } from '$app/navigation';
	import { onMount } from 'svelte';
	import { appLocale } from '@opensession/ui';

	onMount(() => {
		void goto('/login', { replaceState: true });
	});
</script>

<div class="py-16 text-center text-xs text-text-muted">
	{$appLocale === 'ko' ? '로그인으로 이동하는 중...' : 'Redirecting to login...'}
</div>
