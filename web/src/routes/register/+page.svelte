<script lang="ts">
	import { goto } from '$app/navigation';
	import { onMount } from 'svelte';

	onMount(() => {
		void goto('/login', { replaceState: true });
	});
</script>

<div class="py-16 text-center text-xs text-text-muted">Redirecting to login...</div>
