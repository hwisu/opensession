<script lang="ts">
	import { RegisterPage } from '@opensession/ui/components';
	import { goto } from '$app/navigation';
	import { onMount } from 'svelte';
	import { isAuthApiAvailable } from '$lib/api';

	onMount(() => {
		let cancelled = false;
		void isAuthApiAvailable().then((enabled) => {
			if (!enabled && !cancelled) {
				void goto('/');
			}
		});
		return () => {
			cancelled = true;
		};
	});
</script>

<RegisterPage onSuccess={() => goto('/')} onNavigate={(path) => goto(path)} />
