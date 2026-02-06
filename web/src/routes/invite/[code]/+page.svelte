<script lang="ts">
	import { page } from '$app/stores';
	import { getInviteInfo, joinInvite } from '$lib/api';
	import { goto } from '$app/navigation';
	import type { InviteInfo } from '$lib/types';

	let invite = $state<InviteInfo | null>(null);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let joining = $state(false);
	let joinError = $state<string | null>(null);

	$effect(() => {
		const code = $page.params.code;
		loading = true;
		error = null;
		getInviteInfo(code)
			.then((info) => {
				invite = info;
			})
			.catch((e) => {
				error = e instanceof Error ? e.message : 'Invalid or expired invite';
			})
			.finally(() => {
				loading = false;
			});
	});

	async function handleJoin() {
		const code = $page.params.code;
		joining = true;
		joinError = null;
		try {
			const result = await joinInvite(code);
			goto(`/groups/${result.group_id}`);
		} catch (e) {
			joinError = e instanceof Error ? e.message : 'Failed to join group';
		} finally {
			joining = false;
		}
	}
</script>

<svelte:head>
	<title>Join Group - opensession.io</title>
</svelte:head>

<div class="mx-auto max-w-md py-16">
	{#if loading}
		<div class="text-center text-sm text-text-muted">Loading invite...</div>
	{:else if error}
		<div class="text-center">
			<p class="text-error">{error}</p>
			<a href="/" class="mt-2 inline-block text-sm text-accent hover:underline">Go home</a>
		</div>
	{:else if invite}
		<div class="rounded-lg border border-border bg-bg-secondary p-6 text-center">
			<p class="mb-2 text-sm text-text-muted">You have been invited to join</p>
			<h1 class="text-xl font-bold text-white">{invite.group_name}</h1>
			{#if invite.group_description}
				<p class="mt-2 text-sm text-text-secondary">{invite.group_description}</p>
			{/if}
			<p class="mt-3 text-xs text-text-muted">{invite.member_count} members</p>
			{#if invite.inviter}
				<p class="mt-1 text-xs text-text-muted">Invited by {invite.inviter}</p>
			{/if}

			{#if joinError}
				<p class="mt-3 text-sm text-error">{joinError}</p>
			{/if}

			<button
				onclick={handleJoin}
				disabled={joining}
				class="mt-6 w-full rounded-lg bg-accent px-4 py-2.5 text-sm font-medium text-white transition-colors hover:bg-accent/80 disabled:opacity-50"
			>
				{joining ? 'Joining...' : 'Join Group'}
			</button>
		</div>
	{/if}
</div>
