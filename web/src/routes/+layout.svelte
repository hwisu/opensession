<script lang="ts">
	import '../app.css';
	import type { Snippet } from 'svelte';
	import { AppShell } from '@opensession/ui/components';
	import { isAuthenticated } from '@opensession/ui/api';
	import { page } from '$app/stores';

	let { children }: { children: Snippet } = $props();

	let isLanding = $derived($page.url.pathname === '/' && !isAuthenticated());
</script>

{#if isLanding}
	{@render children()}
{:else}
	<AppShell currentPath={$page.url.pathname}>
		{@render children()}
	</AppShell>
{/if}
