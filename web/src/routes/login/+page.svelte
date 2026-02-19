<script lang="ts">
	import { LoginPage } from '@opensession/ui/components';
	import { goto } from '$app/navigation';
	import { onMount } from 'svelte';
	import { appProfile } from '$lib/profile';

	onMount(() => {
		if (appProfile === 'worker') {
			void goto('/');
		}
	});
</script>

<LoginPage onSuccess={() => goto('/')} onNavigate={(path) => goto(path)} />
