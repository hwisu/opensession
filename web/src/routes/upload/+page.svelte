<script lang="ts">
	import { isUploadApiAvailable } from '@opensession/ui';
	import { UploadPage } from '@opensession/ui/components';
	import { goto } from '$app/navigation';
	import { onMount } from 'svelte';

	let checkingCapability = $state(true);
	let uploadEnabled = $state(false);

	onMount(() => {
		let cancelled = false;
		void isUploadApiAvailable()
			.then((enabled) => {
				if (cancelled) return;
				uploadEnabled = enabled;
			})
			.catch(() => {
				if (cancelled) return;
				uploadEnabled = false;
			})
			.finally(() => {
				if (!cancelled) checkingCapability = false;
			});

		return () => {
			cancelled = true;
		};
	});
</script>

{#if checkingCapability}
	<div class="mx-auto max-w-2xl border border-border bg-bg-secondary p-6 text-sm text-text-secondary">
		Checking upload capability...
	</div>
{:else if !uploadEnabled}
	<div class="mx-auto max-w-2xl border border-border bg-bg-secondary p-6 text-sm text-text-secondary">
		Uploads are read-only in this deployment.
		<a href="/" class="ml-1 underline">Back to sessions</a>
	</div>
{:else}
	<UploadPage onSuccess={(id) => goto(`/session/${id}`)} />
{/if}
