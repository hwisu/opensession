<script lang="ts">
	import '../app.css';
	import type { Snippet } from 'svelte';
	import { AppShell } from '@opensession/ui/components';
	import { page } from '$app/stores';
	import { goto } from '$app/navigation';

	let { children }: { children: Snippet } = $props();
</script>

<AppShell currentPath={$page.url.pathname} onNavigate={(path) => goto(path)}>
	{@render children()}
</AppShell>
