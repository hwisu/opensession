<script lang="ts">
	import { LandingPage } from '@opensession/ui/components';
	import { goto } from '$app/navigation';
</script>

<LandingPage onNavigate={(path) => goto(path)} />
