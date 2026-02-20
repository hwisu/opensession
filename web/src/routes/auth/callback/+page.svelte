<script lang="ts">
	import { AuthCallbackPage } from '@opensession/ui/components';
	import { goto } from '$app/navigation';
</script>

<AuthCallbackPage onSuccess={() => goto('/sessions')} onError={() => goto('/login')} />
