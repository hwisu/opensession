<script lang="ts">
	import { listTeams, createTeam } from '$lib/api';
	import type { TeamResponse } from '$lib/types';

	let teams = $state<TeamResponse[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let showCreateForm = $state(false);

	// Create form state
	let newName = $state('');
	let newDescription = $state('');
	let creating = $state(false);
	let createError = $state<string | null>(null);

	async function fetchTeams() {
		loading = true;
		error = null;
		try {
			const res = await listTeams();
			teams = res.teams;
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load teams';
		} finally {
			loading = false;
		}
	}

	async function handleCreate() {
		if (!newName.trim()) return;
		creating = true;
		createError = null;
		try {
			const team = await createTeam({
				name: newName.trim(),
				description: newDescription.trim() || undefined
			});
			teams = [team, ...teams];
			showCreateForm = false;
			newName = '';
			newDescription = '';
		} catch (e) {
			createError = e instanceof Error ? e.message : 'Failed to create team';
		} finally {
			creating = false;
		}
	}

	$effect(() => {
		fetchTeams();
	});
</script>

<svelte:head>
	<title>Teams - opensession.io</title>
</svelte:head>

<div>
	<div class="mb-6 flex items-center justify-between">
		<div>
			<h1 class="text-2xl font-bold text-white">Teams</h1>
			<p class="mt-1 text-sm text-text-secondary">
				Collaborate with your team and share sessions
			</p>
		</div>
		<button
			onclick={() => (showCreateForm = !showCreateForm)}
			class="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-accent/80"
		>
			{showCreateForm ? 'Cancel' : 'Create Team'}
		</button>
	</div>

	{#if showCreateForm}
		<div class="mb-6 rounded-lg border border-border bg-bg-secondary p-4">
			<h3 class="mb-3 text-sm font-medium text-text-primary">Create New Team</h3>
			<div class="space-y-3">
				<input
					type="text"
					placeholder="Team name"
					bind:value={newName}
					class="w-full rounded-lg border border-border bg-bg-primary px-3 py-2 text-sm text-text-primary placeholder-text-muted outline-none focus:border-accent"
				/>
				<textarea
					placeholder="Description (optional)"
					bind:value={newDescription}
					rows={2}
					class="w-full rounded-lg border border-border bg-bg-primary px-3 py-2 text-sm text-text-primary placeholder-text-muted outline-none focus:border-accent"
				></textarea>
				{#if createError}
					<p class="text-sm text-error">{createError}</p>
				{/if}
				<button
					onclick={handleCreate}
					disabled={creating || !newName.trim()}
					class="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-accent/80 disabled:opacity-50"
				>
					{creating ? 'Creating...' : 'Create'}
				</button>
			</div>
		</div>
	{/if}

	{#if error}
		<div class="mb-4 rounded-lg border border-error/30 bg-error/10 px-4 py-3 text-sm text-error">
			{error}
		</div>
	{/if}

	{#if loading}
		<div class="py-16 text-center text-sm text-text-muted">Loading teams...</div>
	{:else if teams.length === 0}
		<div class="py-16 text-center">
			<p class="text-lg text-text-muted">No teams yet</p>
			<p class="mt-1 text-sm text-text-muted">Create a team to start collaborating</p>
		</div>
	{:else}
		<div class="grid gap-3 sm:grid-cols-2">
			{#each teams as team (team.id)}
				<a
					href="/teams/{team.id}"
					class="rounded-lg border border-border bg-bg-secondary p-4 transition-colors hover:border-border-light hover:bg-bg-tertiary"
				>
					<h3 class="text-sm font-medium text-text-primary">{team.name}</h3>
					{#if team.description}
						<p class="mt-1 text-xs text-text-secondary line-clamp-2">{team.description}</p>
					{/if}
					<div class="mt-3 text-xs text-text-muted">
						<span>Created {team.created_at.slice(0, 10)}</span>
					</div>
				</a>
			{/each}
		</div>
	{/if}
</div>
