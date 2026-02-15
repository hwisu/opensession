<script lang="ts">
import { onMount } from 'svelte';
import { isAuthenticated, verifyAuth } from '../api';
import LandingPage from './LandingPage.svelte';
import SessionListPage from './SessionListPage.svelte';

const { onNavigate }: { onNavigate: (path: string) => void } = $props();

let authed = $state(false);

onMount(() => {
	authed = isAuthenticated();
	void verifyAuth()
		.then((ok) => {
			authed = ok;
		})
		.catch(() => {
			authed = false;
		});
});
</script>

<div class="h-full">
	{#if authed}
		<SessionListPage {onNavigate} />
	{:else}
		<LandingPage {onNavigate} />
	{/if}
</div>
