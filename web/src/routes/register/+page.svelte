<script lang="ts">
	import { RegisterPage } from '@opensession/ui/components';
	import { goto } from '$app/navigation';
	import { onMount } from 'svelte';
	import { appProfile } from '$lib/profile';

	onMount(() => {
		if (appProfile === 'worker') {
			void goto('/');
		}
	});
</script>

<RegisterPage onSuccess={() => goto('/')} onNavigate={(path) => goto(path)} />
