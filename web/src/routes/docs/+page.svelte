<script lang="ts">
	import { isUploadApiAvailable } from '@opensession/ui';
	import { DocsPage } from '@opensession/ui/components';
	import { goto } from '$app/navigation';
	import { onMount } from 'svelte';

	let uploadEnabled = $state(false);

	onMount(() => {
		let cancelled = false;
		void isUploadApiAvailable()
			.then((enabled) => {
				if (!cancelled) uploadEnabled = enabled;
			})
			.catch(() => {
				if (!cancelled) uploadEnabled = false;
			});
		return () => {
			cancelled = true;
		};
	});
</script>

<DocsPage onNavigate={(path) => goto(path)} showUploadLink={uploadEnabled} />
