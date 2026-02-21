<script lang="ts">
import LandingPage from './LandingPage.svelte';
import SessionListPage from './SessionListPage.svelte';

const {
	onNavigate,
}: {
	onNavigate: (path: string) => void;
} = $props();
</script>

<div class="h-full space-y-8">
	<LandingPage {onNavigate} />
	<SessionListPage {onNavigate} />
</div>
