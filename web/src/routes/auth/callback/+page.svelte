<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/stores';
	import { setApiKey } from '$lib/api';

	$effect(() => {
		const apiKey = $page.url.searchParams.get('api_key');
		if (apiKey) {
			setApiKey(apiKey);
			goto('/settings', { replaceState: true });
		} else {
			goto('/settings?error=missing_key', { replaceState: true });
		}
	});
</script>

<div class="py-16 text-center text-sm text-text-muted">Signing in...</div>
