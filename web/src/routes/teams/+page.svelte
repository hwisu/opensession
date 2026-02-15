<script lang="ts">
	import { TeamsListPage } from '@opensession/ui/components';
	import { goto } from '$app/navigation';
</script>

<TeamsListPage onNavigate={(path) => goto(path)} />
