<script lang="ts">
	import { DocsPage } from '@opensession/ui/components';
	import { goto } from '$app/navigation';
</script>

<DocsPage onNavigate={(path) => goto(path)} />
