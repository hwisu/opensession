<script lang="ts">
	import { page } from '$app/stores';
	import { getTeam } from '$lib/api';
	import SessionCard from '$lib/components/SessionCard.svelte';
	import type { TeamDetailResponse } from '$lib/types';

	let team = $state<TeamDetailResponse | null>(null);
	let loading = $state(true);
	let error = $state<string | null>(null);

	$effect(() => {
		const id = $page.params.id!;
		loading = true;
		error = null;
		getTeam(id)
			.then((t) => {
				team = t;
			})
			.catch((e) => {
				error = e instanceof Error ? e.message : 'Failed to load team';
			})
			.finally(() => {
				loading = false;
			});
	});
</script>

<svelte:head>
	<title>{team?.name ?? 'Team'} - opensession.io</title>
</svelte:head>

{#if loading}
	<div class="py-16 text-center text-sm text-text-muted">Loading team...</div>
{:else if error}
	<div class="py-16 text-center">
		<p class="text-error">{error}</p>
		<a href="/teams" class="mt-2 inline-block text-sm text-accent hover:underline">Back to teams</a>
	</div>
{:else if team}
	<div>
		<a href="/teams" class="mb-4 inline-block text-sm text-text-muted hover:text-text-secondary">
			&larr; Back to teams
		</a>

		<div class="mb-6 rounded-lg border border-border bg-bg-secondary p-5">
			<h1 class="text-xl font-bold text-white">{team.name}</h1>
			{#if team.description}
				<p class="mt-1 text-sm text-text-secondary">{team.description}</p>
			{/if}

			<div class="mt-3 flex gap-4 text-sm text-text-muted">
				<span>{team.member_count} members</span>
				<span>{team.sessions.length} sessions</span>
			</div>
		</div>

		<!-- Sessions -->
		<div>
			<h2 class="mb-3 text-sm font-medium text-text-primary">Sessions</h2>
			{#if team.sessions.length === 0}
				<p class="py-8 text-center text-sm text-text-muted">No sessions shared yet</p>
			{:else}
				<div class="grid gap-3">
					{#each team.sessions as session (session.id)}
						<SessionCard {session} />
					{/each}
				</div>
			{/if}
		</div>
	</div>
{/if}
