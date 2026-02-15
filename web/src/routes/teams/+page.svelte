<script lang="ts">
	import { TeamsListPage } from '@opensession/ui/components';
	import { goto } from '$app/navigation';
	import { isWorkerProfile } from '$lib/profile';
</script>

{#if isWorkerProfile}
	<div class="mx-auto max-w-2xl border border-border bg-bg-secondary p-6 text-sm text-text-secondary">
		Teams are disabled in this deployment profile.
		<a href="/" class="ml-1 underline">Back to sessions</a>
	</div>
{:else}
	<TeamsListPage onNavigate={(path) => goto(path)} />
{/if}
