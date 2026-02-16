<script lang="ts">
	import { HomePage } from '@opensession/ui/components';
	import { goto } from '$app/navigation';
	import { isWorkerProfile } from '$lib/profile';
</script>

<HomePage onNavigate={(path) => goto(path)} showLandingForGuests={isWorkerProfile} />
