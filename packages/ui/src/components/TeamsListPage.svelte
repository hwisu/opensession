<script lang="ts">
import { ApiError, createTeam, isAuthenticated, listTeams } from '../api';
import type { TeamResponse } from '../types';
import AuthGuideCard from './AuthGuideCard.svelte';

const {
	onNavigate = (path: string) => {
		if (typeof window !== 'undefined') window.location.href = path;
	},
}: {
	onNavigate?: (path: string) => void;
} = $props();

let teams = $state<TeamResponse[]>([]);
let loading = $state(true);
let error = $state<string | null>(null);
let unauthorized = $state(false);
let showCreateForm = $state(false);

let newName = $state('');
let newDescription = $state('');
let creating = $state(false);
let createError = $state<string | null>(null);

async function fetchTeams() {
	loading = true;
	error = null;
	unauthorized = false;
	try {
		const res = await listTeams();
		teams = res.teams;
	} catch (e) {
		if (e instanceof ApiError && (e.status === 401 || e.status === 403)) {
			unauthorized = true;
			return;
		}
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
			description: newDescription.trim() || undefined,
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
	if (!isAuthenticated()) {
		loading = false;
		unauthorized = true;
		return;
	}
	fetchTeams();
});
</script>

<svelte:head>
	<title>Teams - opensession.io</title>
</svelte:head>

<div class="flex h-full flex-col">
	<div class="flex shrink-0 items-center justify-between border-b border-border px-3 py-1.5">
		<span class="text-xs text-text-muted">Teams ({teams.length})</span>
		{#if !unauthorized}
			<button
				onclick={() => (showCreateForm = !showCreateForm)}
				class="px-2 py-0.5 text-xs text-accent transition-colors hover:text-accent/80"
			>
				{showCreateForm ? '[Cancel]' : '[Create Team]'}
			</button>
		{/if}
	</div>

	{#if showCreateForm && !unauthorized}
		<div class="shrink-0 border-b border-border p-3">
			<h3 class="mb-2 text-sm font-medium text-text-primary">Create New Team</h3>
			<div class="space-y-2">
				<div>
					<label for="team-name" class="sr-only">Team name</label>
					<input
						id="team-name"
						type="text"
						placeholder="Team name"
						bind:value={newName}
						class="w-full border border-border bg-bg-primary px-3 py-1.5 text-xs text-text-primary placeholder-text-muted outline-none focus:border-accent"
					/>
				</div>
				<div>
					<label for="team-desc" class="sr-only">Description</label>
					<textarea
						id="team-desc"
						placeholder="Description (optional)"
						bind:value={newDescription}
						rows={2}
						class="w-full border border-border bg-bg-primary px-3 py-1.5 text-xs text-text-primary placeholder-text-muted outline-none focus:border-accent"
					></textarea>
				</div>
				{#if createError}
					<p class="text-xs text-error">{createError}</p>
				{/if}
				<button
					onclick={handleCreate}
					disabled={creating || !newName.trim()}
					class="bg-accent px-3 py-1 text-xs font-medium text-white transition-colors hover:bg-accent/80 disabled:opacity-50"
				>
					{creating ? 'Creating...' : 'Create'}
				</button>
			</div>
		</div>
	{/if}

	{#if error}
		<div class="border-b border-error/30 bg-error/10 px-3 py-2 text-xs text-error">
			{error}
		</div>
	{/if}

	<div class="flex-1 overflow-y-auto">
		{#if unauthorized}
			<div class="p-3">
				<AuthGuideCard
					title="Teams need an account"
					description="Create or join teams after signing in. Public sessions are still visible from the Sessions menu."
					{onNavigate}
				/>
			</div>
		{:else if loading}
			<div class="py-8 text-center text-xs text-text-muted">Loading teams...</div>
		{:else if teams.length === 0}
			<div class="py-8 text-center">
				<p class="text-xs text-text-muted">No teams yet</p>
				<p class="mt-1 text-xs text-text-muted">Create a team to start collaborating</p>
			</div>
		{:else}
			{#each teams as team (team.id)}
				<a
					href="/teams/{team.id}"
					class="flex items-center gap-3 border-b border-border px-3 py-2 text-xs transition-colors hover:bg-bg-hover"
				>
					<span class="font-medium text-text-primary">{team.name}</span>
					{#if team.description}
						<span class="min-w-0 flex-1 truncate text-text-muted">{team.description}</span>
					{/if}
					<span class="shrink-0 text-text-muted">{team.created_at.slice(0, 10)}</span>
				</a>
			{/each}
		{/if}
	</div>
</div>
