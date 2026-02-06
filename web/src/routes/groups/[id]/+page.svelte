<script lang="ts">
	import { page } from '$app/stores';
	import { getGroup } from '$lib/api';
	import SessionCard from '$lib/components/SessionCard.svelte';
	import type { GroupDetailResponse } from '$lib/types';

	let group = $state<GroupDetailResponse | null>(null);
	let loading = $state(true);
	let error = $state<string | null>(null);

	$effect(() => {
		const id = $page.params.id;
		loading = true;
		error = null;
		getGroup(id)
			.then((g) => {
				group = g;
			})
			.catch((e) => {
				error = e instanceof Error ? e.message : 'Failed to load group';
			})
			.finally(() => {
				loading = false;
			});
	});
</script>

<svelte:head>
	<title>{group?.name ?? 'Group'} - opensession.io</title>
</svelte:head>

{#if loading}
	<div class="py-16 text-center text-sm text-text-muted">Loading group...</div>
{:else if error}
	<div class="py-16 text-center">
		<p class="text-error">{error}</p>
		<a href="/groups" class="mt-2 inline-block text-sm text-accent hover:underline">Back to groups</a>
	</div>
{:else if group}
	<div>
		<a href="/groups" class="mb-4 inline-block text-sm text-text-muted hover:text-text-secondary">
			&larr; Back to groups
		</a>

		<div class="mb-6 rounded-lg border border-border bg-bg-secondary p-5">
			<div class="flex items-start justify-between">
				<div>
					<h1 class="text-xl font-bold text-white">{group.name}</h1>
					{#if group.description}
						<p class="mt-1 text-sm text-text-secondary">{group.description}</p>
					{/if}
				</div>
			</div>

			<div class="mt-3 flex gap-4 text-sm text-text-muted">
				<span>{group.member_count} members</span>
				<span>{group.sessions.length} sessions</span>
				{#if group.is_public}
					<span class="text-success">Public</span>
				{:else}
					<span>Private</span>
				{/if}
			</div>
		</div>

		<!-- Sessions -->
		<div>
			<h2 class="mb-3 text-sm font-medium text-text-primary">Sessions</h2>
			{#if group.sessions.length === 0}
				<p class="py-8 text-center text-sm text-text-muted">No sessions shared yet</p>
			{:else}
				<div class="grid gap-3">
					{#each group.sessions as session (session.id)}
						<SessionCard {session} />
					{/each}
				</div>
			{/if}
		</div>
	</div>
{/if}
