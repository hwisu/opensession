<script lang="ts">
	import { RegisterPage } from '@opensession/ui/components';
	import { goto } from '$app/navigation';
</script>

<RegisterPage onSuccess={() => goto('/')} onNavigate={(path) => goto(path)} />
