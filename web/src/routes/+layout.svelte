<script lang="ts">
	import '../app.css';
	import type { Snippet } from 'svelte';
	import { AppShell } from '@opensession/ui/components';
	import { page } from '$app/stores';

	let { children }: { children: Snippet } = $props();
</script>

<AppShell currentPath={$page.url.pathname}>
	{@render children()}
</AppShell>
