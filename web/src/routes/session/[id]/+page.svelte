<script lang="ts">
	import { SessionDetailPage } from '@opensession/ui/components';
	import { page } from '$app/stores';
</script>

<SessionDetailPage sessionId={$page.params.id!} />
