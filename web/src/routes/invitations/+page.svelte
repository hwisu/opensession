<script lang="ts">
	import { InvitationsPage } from '@opensession/ui/components';
	import { goto } from '$app/navigation';
</script>

<InvitationsPage onNavigate={(path) => goto(path)} />
