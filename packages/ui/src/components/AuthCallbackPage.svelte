<script lang="ts">
import { handleAuthCallback } from '../api';

const {
	onSuccess = () => {},
	onError = () => {},
}: {
	onSuccess?: () => void;
	onError?: () => void;
} = $props();

let status = $state<'loading' | 'error'>('loading');

$effect(() => {
	const tokens = handleAuthCallback();
	if (tokens) {
		onSuccess();
	} else {
		status = 'error';
		onError();
	}
});
</script>

<div class="flex items-center justify-center pt-24">
	{#if status === 'loading'}
		<p class="text-xs text-text-muted">Completing sign in...</p>
	{:else}
		<p class="text-xs text-error">Authentication failed. Please try again.</p>
	{/if}
</div>
