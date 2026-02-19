<script lang="ts">
import { ApiError, authLogout, getSettings, isAuthenticated, verifyAuth } from '../api';
import LandingPage from './LandingPage.svelte';
import SessionListPage from './SessionListPage.svelte';

const {
	onNavigate,
	showLandingForGuests = false,
	uploadEnabled = true,
	authEnabled = true,
}: {
	onNavigate: (path: string) => void;
	showLandingForGuests?: boolean;
	uploadEnabled?: boolean;
	authEnabled?: boolean;
} = $props();

let authed = $state(false);

$effect(() => {
	if (!showLandingForGuests) {
		authed = true;
		return;
	}

	authed = false;
	if (!authEnabled) return;
	if (!isAuthenticated()) return;

	let cancelled = false;
	verifyAuth()
		.then(async (ok) => {
			if (!ok || cancelled) {
				authed = false;
				return;
			}
			try {
				await getSettings();
				if (!cancelled) authed = true;
			} catch (e) {
				if (e instanceof ApiError && (e.status === 401 || e.status === 403)) {
					await authLogout();
				}
				if (!cancelled) authed = false;
			}
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
