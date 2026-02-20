<script lang="ts">
	import { LoginPage } from '@opensession/ui/components';
	import { goto } from '$app/navigation';
</script>

<LoginPage onSuccess={() => goto('/sessions')} />
