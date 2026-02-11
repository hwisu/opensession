<script lang="ts">
	import { SessionListPage, LandingPage } from '@opensession/ui/components';
	import { isAuthenticated } from '@opensession/ui/api';
	import { goto } from '$app/navigation';

	let authed = $state(isAuthenticated());
</script>

{#if authed}
	<SessionListPage onNavigate={(path) => goto(path)} />
{:else}
	<LandingPage onNavigate={(path) => goto(path)} />
{/if}
