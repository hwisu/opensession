<script lang="ts">
	import { SessionListPage } from '@opensession/ui/components';
	import { goto } from '$app/navigation';
</script>

<SessionListPage onNavigate={(path) => goto(path)} />
