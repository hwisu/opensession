<script lang="ts">
import LandingPage from './LandingPage.svelte';
import SessionListPage from './SessionListPage.svelte';

const {
	onNavigate,
	uploadEnabled = true,
}: {
	onNavigate: (path: string) => void;
	uploadEnabled?: boolean;
} = $props();
</script>

<div class="h-full space-y-8">
	<LandingPage {onNavigate} />
	<SessionListPage {onNavigate} {uploadEnabled} />
</div>
