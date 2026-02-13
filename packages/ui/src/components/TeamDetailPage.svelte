<script lang="ts">
import { getSettings, getTeam, inviteMember, listMembers } from '../api';
import type { MemberResponse, TeamDetailResponse, UserSettings } from '../types';
import SessionCard from './SessionCard.svelte';

const { teamId }: { teamId: string } = $props();

let team = $state<TeamDetailResponse | null>(null);
let members = $state<MemberResponse[]>([]);
let currentUser = $state<UserSettings | null>(null);
let loading = $state(true);
let error = $state<string | null>(null);

// Invite form
let inviteTarget = $state('');
let inviteType = $state<'email' | 'oauth'>('email');
let inviteProvider = $state('github');
let inviteRole = $state('member');
let inviting = $state(false);
let inviteError = $state<string | null>(null);
let inviteSuccess = $state(false);

const isTeamAdmin = $derived(
	currentUser != null &&
		members.some((m) => m.user_id === currentUser?.user_id && m.role === 'admin'),
);

async function fetchData() {
	loading = true;
	error = null;
	try {
		const [t, m] = await Promise.all([getTeam(teamId), listMembers(teamId)]);
		team = t;
		members = m.members;
		try {
			currentUser = await getSettings();
		} catch {
			currentUser = null;
		}
	} catch (e) {
		error = e instanceof Error ? e.message : 'Failed to load team';
	} finally {
		loading = false;
	}
}

async function handleInvite() {
	if (!inviteTarget.trim()) return;
	inviting = true;
	inviteError = null;
	inviteSuccess = false;
	try {
		const data =
			inviteType === 'email'
				? { email: inviteTarget.trim(), role: inviteRole }
				: {
						oauth_provider: inviteProvider,
						oauth_provider_username: inviteTarget.trim(),
						role: inviteRole,
					};
		await inviteMember(teamId, data);
		inviteSuccess = true;
		inviteTarget = '';
	} catch (e) {
		if (e instanceof Error) {
			try {
				const parsed = JSON.parse(e.message);
				inviteError = parsed.error || e.message;
			} catch {
				inviteError = e.message;
			}
		} else {
			inviteError = 'Failed to send invitation';
		}
	} finally {
		inviting = false;
	}
}

$effect(() => {
	fetchData();
});
</script>

<svelte:head>
	<title>{team?.name ?? 'Team'} - opensession.io</title>
</svelte:head>

{#if loading}
	<div class="py-8 text-center text-xs text-text-muted">Loading team...</div>
{:else if error}
	<div class="py-8 text-center">
		<p class="text-xs text-error">{error}</p>
		<a href="/teams" class="mt-2 inline-block text-xs text-accent hover:underline">Back to teams</a>
	</div>
{:else if team}
	<div class="flex h-full flex-col">
		<div class="shrink-0 border-b border-border px-3 py-2">
			<div class="flex items-center gap-2">
				<a href="/teams" class="text-xs text-text-muted hover:text-text-secondary">&larr;</a>
				<h1 class="text-lg font-bold text-text-primary">{team.name}</h1>
			</div>
			{#if team.description}
				<p class="mt-1 text-xs text-text-secondary">{team.description}</p>
			{/if}
			<div class="mt-1 flex gap-3 text-xs text-text-muted">
				<span>{members.length} members</span>
				<span>{team.sessions.length} sessions</span>
			</div>
		</div>

		<div class="flex-1 overflow-y-auto">
			<!-- Members -->
			<div class="border-b border-border px-3 py-1 text-xs text-text-muted">
				Members ({members.length})
			</div>
			<div class="border-b border-border">
				{#each members as member (member.user_id)}
					<div class="flex items-center justify-between px-3 py-1.5">
						<span class="text-xs text-text-primary">{member.nickname}</span>
						<span class="text-xs text-text-muted">{member.role}</span>
					</div>
				{/each}
			</div>

			<!-- Invite Form (team admin only) -->
			{#if isTeamAdmin}
				<div class="border-b border-border px-3 py-2">
					<h3 class="mb-2 text-sm font-medium text-text-primary">Invite Member</h3>
					<form onsubmit={(e) => { e.preventDefault(); handleInvite(); }} class="flex flex-wrap items-end gap-2">
						<div class="flex gap-1">
							<button
								type="button"
								onclick={() => { inviteType = 'email'; }}
								class="px-2 py-1 text-xs"
								class:bg-accent={inviteType === 'email'}
								class:text-white={inviteType === 'email'}
								class:bg-bg-hover={inviteType !== 'email'}
								class:text-text-secondary={inviteType !== 'email'}
							>
								Email
							</button>
							<button
								type="button"
								onclick={() => { inviteType = 'oauth'; }}
								class="px-2 py-1 text-xs"
								class:bg-accent={inviteType === 'oauth'}
								class:text-white={inviteType === 'oauth'}
								class:bg-bg-hover={inviteType !== 'oauth'}
								class:text-text-secondary={inviteType !== 'oauth'}
							>
								OAuth
							</button>
						</div>
						{#if inviteType === 'oauth'}
							<select
								bind:value={inviteProvider}
								class="border border-border bg-bg-primary px-2 py-1 text-xs text-text-primary outline-none"
							>
								<option value="github">GitHub</option>
								<option value="gitlab">GitLab</option>
							</select>
						{/if}
						<input
							type="text"
							placeholder={inviteType === 'email' ? 'user@example.com' : 'username'}
							bind:value={inviteTarget}
							class="flex-1 border border-border bg-bg-primary px-3 py-1 text-xs text-text-primary placeholder-text-muted outline-none focus:border-accent"
						/>
						<select
							bind:value={inviteRole}
							class="border border-border bg-bg-primary px-2 py-1 text-xs text-text-primary outline-none"
						>
							<option value="member">member</option>
							<option value="admin">admin</option>
						</select>
						<button
							type="submit"
							disabled={inviting || !inviteTarget.trim()}
							class="bg-accent px-3 py-1 text-xs text-white hover:bg-accent/80 disabled:opacity-50"
						>
							{inviting ? 'Sending...' : 'Invite'}
						</button>
					</form>
					{#if inviteError}
						<p class="mt-1 text-xs text-error">{inviteError}</p>
					{/if}
					{#if inviteSuccess}
						<p class="mt-1 text-xs text-success">Invitation sent!</p>
					{/if}
				</div>
			{/if}

			<!-- Sessions -->
			<div class="border-b border-border px-3 py-1 text-xs text-text-muted">
				Sessions ({team.sessions.length})
			</div>
			{#if team.sessions.length === 0}
				<p class="py-8 text-center text-xs text-text-muted">No sessions shared yet</p>
			{:else}
				{#each team.sessions as session (session.id)}
					<SessionCard {session} />
				{/each}
			{/if}
		</div>
	</div>
{/if}
