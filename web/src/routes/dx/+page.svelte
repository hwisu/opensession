<script lang="ts">
	import { goto } from '$app/navigation';
	import { DxLabPage } from '@opensession/ui/components';
</script>

<DxLabPage onNavigate={(path) => goto(path)} />
