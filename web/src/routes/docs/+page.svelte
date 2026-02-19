<script lang="ts">
	import { DocsPage } from '@opensession/ui/components';
	import { goto } from '$app/navigation';
	import { onMount } from 'svelte';
	import { isAuthApiAvailable } from '$lib/api';
	import { appProfile } from '$lib/profile';

	let authEnabled = $state(appProfile === 'server');

	onMount(() => {
		let cancelled = false;
		void isAuthApiAvailable().then((enabled) => {
			if (!cancelled) authEnabled = enabled;
		});
		return () => {
			cancelled = true;
		};
	});
</script>

<DocsPage onNavigate={(path) => goto(path)} showUploadLink={authEnabled} />
