<script lang="ts">
import SessionListPage from './SessionListPage.svelte';

const {
	onNavigate,
	uploadEnabled = true,
}: {
	onNavigate: (path: string) => void;
	uploadEnabled?: boolean;
} = $props();
</script>

<div class="h-full">
	<SessionListPage {onNavigate} {uploadEnabled} />
</div>
