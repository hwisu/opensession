<script lang="ts">
	import { getApiCapabilities } from '@opensession/ui';
	import { SessionListPage } from '@opensession/ui/components';
	import { goto } from '$app/navigation';
	import { onMount } from 'svelte';

	let uploadEnabled = $state(false);

	onMount(() => {
		let cancelled = false;
		void getApiCapabilities()
			.then((capabilities) => {
				if (!cancelled) uploadEnabled = capabilities.upload_enabled;
			})
			.catch(() => {
				if (!cancelled) uploadEnabled = false;
			});

		return () => {
			cancelled = true;
		};
	});
</script>

<SessionListPage onNavigate={(path) => goto(path)} {uploadEnabled} />
