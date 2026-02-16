<script lang="ts">
import SessionListPage from './SessionListPage.svelte';

const { onNavigate }: { onNavigate: (path: string) => void } = $props();
</script>

<div class="h-full">
	<SessionListPage {onNavigate} />
</div>
