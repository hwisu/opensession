<script lang="ts">
	import { listGroups, createGroup } from '$lib/api';
	import type { GroupResponse } from '$lib/types';

	let groups = $state<GroupResponse[]>([]);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let showCreateForm = $state(false);

	// Create form state
	let newName = $state('');
	let newDescription = $state('');
	let newIsPublic = $state(true);
	let creating = $state(false);
	let createError = $state<string | null>(null);

	async function fetchGroups() {
		loading = true;
		error = null;
		try {
			const res = await listGroups();
			groups = res.groups;
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load groups';
		} finally {
			loading = false;
		}
	}

	async function handleCreate() {
		if (!newName.trim()) return;
		creating = true;
		createError = null;
		try {
			const group = await createGroup({
				name: newName.trim(),
				description: newDescription.trim() || undefined,
				is_public: newIsPublic
			});
			groups = [group, ...groups];
			showCreateForm = false;
			newName = '';
			newDescription = '';
		} catch (e) {
			createError = e instanceof Error ? e.message : 'Failed to create group';
		} finally {
			creating = false;
		}
	}

	$effect(() => {
		fetchGroups();
	});
</script>

<svelte:head>
	<title>Groups - opensession.io</title>
</svelte:head>

<div>
	<div class="mb-6 flex items-center justify-between">
		<div>
			<h1 class="text-2xl font-bold text-white">Groups</h1>
			<p class="mt-1 text-sm text-text-secondary">
				Collaborate with teams and share sessions
			</p>
		</div>
		<button
			onclick={() => (showCreateForm = !showCreateForm)}
			class="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-accent/80"
		>
			{showCreateForm ? 'Cancel' : 'Create Group'}
		</button>
	</div>

	{#if showCreateForm}
		<div class="mb-6 rounded-lg border border-border bg-bg-secondary p-4">
			<h3 class="mb-3 text-sm font-medium text-text-primary">Create New Group</h3>
			<div class="space-y-3">
				<input
					type="text"
					placeholder="Group name"
					bind:value={newName}
					class="w-full rounded-lg border border-border bg-bg-primary px-3 py-2 text-sm text-text-primary placeholder-text-muted outline-none focus:border-accent"
				/>
				<textarea
					placeholder="Description (optional)"
					bind:value={newDescription}
					rows={2}
					class="w-full rounded-lg border border-border bg-bg-primary px-3 py-2 text-sm text-text-primary placeholder-text-muted outline-none focus:border-accent"
				></textarea>
				<label class="flex items-center gap-2 text-sm text-text-secondary">
					<input type="checkbox" bind:checked={newIsPublic} class="accent-accent" />
					Public group
				</label>
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
		<div class="py-16 text-center text-sm text-text-muted">Loading groups...</div>
	{:else if groups.length === 0}
		<div class="py-16 text-center">
			<p class="text-lg text-text-muted">No groups yet</p>
			<p class="mt-1 text-sm text-text-muted">Create a group to start collaborating</p>
		</div>
	{:else}
		<div class="grid gap-3 sm:grid-cols-2">
			{#each groups as group (group.id)}
				<a
					href="/groups/{group.id}"
					class="rounded-lg border border-border bg-bg-secondary p-4 transition-colors hover:border-border-light hover:bg-bg-tertiary"
				>
					<div class="flex items-start justify-between">
						<h3 class="text-sm font-medium text-text-primary">{group.name}</h3>
						{#if group.is_public}
							<span class="rounded-full bg-success/10 px-2 py-0.5 text-xs text-success">Public</span>
						{:else}
							<span class="rounded-full bg-bg-hover px-2 py-0.5 text-xs text-text-muted">Private</span>
						{/if}
					</div>
					{#if group.description}
						<p class="mt-1 text-xs text-text-secondary line-clamp-2">{group.description}</p>
					{/if}
					<div class="mt-3 text-xs text-text-muted">
						<span>Created {group.created_at.slice(0, 10)}</span>
					</div>
				</a>
			{/each}
		</div>
	{/if}
</div>
