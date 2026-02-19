<script lang="ts">
	import { DocsPage } from '@opensession/ui/components';
	import { goto } from '$app/navigation';
	import { appProfile } from '$lib/profile';
</script>

<DocsPage onNavigate={(path) => goto(path)} showUploadLink={appProfile !== 'worker'} />
