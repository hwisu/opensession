<script lang="ts">
	import { TeamDetailPage } from '@opensession/ui/components';
	import { page } from '$app/stores';
</script>

<TeamDetailPage teamId={$page.params.id!} />
