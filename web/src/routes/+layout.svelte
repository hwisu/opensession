<script lang="ts">
	import '../app.css';
	import type { Snippet } from 'svelte';
	import { AppShell } from '@opensession/ui/components';
	import { page } from '$app/stores';
	import { appProfile } from '$lib/profile';

	let { children }: { children: Snippet } = $props();
</script>

<AppShell currentPath={$page.url.pathname} appProfile={appProfile}>
	{@render children()}
</AppShell>
