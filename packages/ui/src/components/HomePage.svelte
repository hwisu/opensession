<script lang="ts">
import { isAuthenticated, verifyAuth } from '../api';
import LandingPage from './LandingPage.svelte';
import SessionListPage from './SessionListPage.svelte';

const {
	onNavigate,
	showLandingForGuests = false,
	uploadEnabled = true,
}: {
	onNavigate: (path: string) => void;
	showLandingForGuests?: boolean;
	uploadEnabled?: boolean;
} = $props();

let authed = $state(isAuthenticated());

$effect(() => {
	if (!showLandingForGuests) {
		authed = true;
		return;
	}

	authed = isAuthenticated();
	let cancelled = false;
	verifyAuth()
		.then((ok) => {
			if (!cancelled) authed = ok;
		})
		.catch(() => {
			if (!cancelled) authed = false;
		});
	return () => {
		cancelled = true;
	};
});
</script>

<div class="h-full">
	{#if showLandingForGuests && !authed}
		<LandingPage {onNavigate} />
	{:else}
		<SessionListPage {onNavigate} {uploadEnabled} />
	{/if}
</div>
