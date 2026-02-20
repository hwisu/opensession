<script lang="ts">
	import { getApiCapabilities } from '@opensession/ui';
	import { HomePage } from '@opensession/ui/components';
	import { goto } from '$app/navigation';
	import { onMount } from 'svelte';

	let uploadEnabled = $state(false);

	onMount(() => {
		let cancelled = false;
		void getApiCapabilities()
			.then((capabilities) => {
				if (cancelled) return;
				uploadEnabled = capabilities.upload_enabled;
			})
			.catch(() => {
				if (cancelled) return;
				uploadEnabled = false;
			});
		return () => {
			cancelled = true;
		};
	});
</script>

<HomePage
	onNavigate={(path) => goto(path)}
	{uploadEnabled}
/>
