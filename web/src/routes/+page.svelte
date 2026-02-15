<script lang="ts">
	import { HomePage } from '@opensession/ui/components';
	import { goto } from '$app/navigation';
</script>

<HomePage onNavigate={(path) => goto(path)} />
