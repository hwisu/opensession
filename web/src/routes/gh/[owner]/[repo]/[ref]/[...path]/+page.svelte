<script lang="ts">
import { goto } from '$app/navigation';
import { page } from '$app/stores';
import { onMount } from 'svelte';

let errorMessage = $state<string | null>(null);

function decodeOrNull(value: string): string | null {
	try {
		return decodeURIComponent(value);
	} catch {
		return null;
	}
}

function buildRedirectTarget(): string | null {
	const owner = decodeOrNull($page.params.owner ?? '');
	const repo = decodeOrNull($page.params.repo ?? '');
	const ref = decodeOrNull($page.params.ref ?? '');
	const path = decodeOrNull($page.params.path ?? '');
	if (!owner || !repo || !ref || !path) return null;

	const params = new URLSearchParams();
	params.set('remote', `https://github.com/${owner}/${repo}`);
	params.set('ref', ref);
	params.set('path', path);

	for (const key of ['view', 'ef', 'nf', 'parser_hint']) {
		const value = $page.url.searchParams.get(key);
		if (value != null && value.length > 0) {
			params.set(key, value);
		}
	}

	return `/git?${params.toString()}`;
}

onMount(() => {
	const target = buildRedirectTarget();
	if (!target) {
		errorMessage = 'Invalid legacy /gh route parameters.';
		return;
	}
	void goto(target, { replaceState: true });
});
</script>

{#if errorMessage}
	<div class="mx-auto max-w-3xl border border-error/30 bg-error/10 px-4 py-3 text-sm text-error">
		{errorMessage}
	</div>
{:else}
	<div class="py-16 text-center text-xs text-text-muted">Redirecting to /git preview...</div>
{/if}
