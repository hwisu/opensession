<script lang="ts">
import {
	ApiError,
	cancelTeamInvitation,
	createTeamInviteKey,
	getSettings,
	getTeam,
	inviteMember,
	isAuthenticated,
	listMembers,
	listTeamInvitations,
	listTeamInviteKeys,
	revokeTeamInviteKey,
} from '../api';
import type {
	InvitationResponse,
	MemberResponse,
	TeamDetailResponse,
	TeamInviteKeySummary,
	UserSettings,
} from '../types';
import AuthGuideCard from './AuthGuideCard.svelte';
import SessionCard from './SessionCard.svelte';

const { teamId }: { teamId: string } = $props();

let team = $state<TeamDetailResponse | null>(null);
let members = $state<MemberResponse[]>([]);
let currentUser = $state<UserSettings | null>(null);
let loading = $state(true);
let error = $state<string | null>(null);
let unauthorized = $state(false);

// Invite form
let inviteTarget = $state('');
let inviteType = $state<'email' | 'oauth'>('email');
let inviteProvider = $state('github');
let inviteRole = $state('member');
let inviting = $state(false);
let inviteError = $state<string | null>(null);
let inviteSuccess = $state(false);
let teamInvitations = $state<InvitationResponse[]>([]);
let teamInvitationsLoading = $state(false);
let inviteKeys = $state<TeamInviteKeySummary[]>([]);
let keysLoading = $state(false);
let keyError = $state<string | null>(null);
let creatingKey = $state(false);
let createdInviteKey = $state<string | null>(null);
let keyRole = $state<'admin' | 'member'>('member');
let keyDays = $state(7);
let showInactiveKeys = $state(false);

const isTeamAdmin = $derived(
	currentUser != null &&
		members.some((m) => m.user_id === currentUser?.user_id && m.role === 'admin'),
);

async function fetchData() {
	loading = true;
	error = null;
	unauthorized = false;
	try {
		const [t, m] = await Promise.all([getTeam(teamId), listMembers(teamId)]);
		team = t;
		members = m.members;
		try {
			currentUser = await getSettings();
		} catch {
			currentUser = null;
		}

		if (
			currentUser != null &&
			members.some((mm) => mm.user_id === currentUser?.user_id && mm.role === 'admin')
		) {
			await refreshTeamInvitations();
			await refreshInviteKeys();
		} else {
			teamInvitations = [];
			inviteKeys = [];
		}
	} catch (e) {
		if (e instanceof ApiError && (e.status === 401 || e.status === 403)) {
			unauthorized = true;
			return;
		}
		error = e instanceof Error ? e.message : 'Failed to load team';
	} finally {
		loading = false;
	}
}

async function refreshTeamInvitations() {
	teamInvitationsLoading = true;
	inviteError = null;
	try {
		const res = await listTeamInvitations(teamId);
		teamInvitations = res.invitations;
	} catch (e) {
		inviteError = e instanceof Error ? e.message : 'Failed to load invitations';
	} finally {
		teamInvitationsLoading = false;
	}
}

async function refreshInviteKeys() {
	keysLoading = true;
	keyError = null;
	try {
		const res = await listTeamInviteKeys(teamId);
		inviteKeys = res.keys;
	} catch (e) {
		keyError = e instanceof Error ? e.message : 'Failed to load invite keys';
	} finally {
		keysLoading = false;
	}
}

async function handleCreateInviteKey() {
	creatingKey = true;
	keyError = null;
	createdInviteKey = null;
	try {
		const res = await createTeamInviteKey(teamId, {
			role: keyRole,
			expires_in_days: keyDays,
		});
		createdInviteKey = res.invite_key;
		await refreshInviteKeys();
	} catch (e) {
		keyError = e instanceof Error ? e.message : 'Failed to create invite key';
	} finally {
		creatingKey = false;
	}
}

async function handleRevokeInviteKey(keyId: string) {
	keyError = null;
	try {
		await revokeTeamInviteKey(teamId, keyId);
		await refreshInviteKeys();
	} catch (e) {
		keyError = e instanceof Error ? e.message : 'Failed to revoke invite key';
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
		await refreshTeamInvitations();
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

async function handleCancelInvitation(invitationId: string) {
	inviteError = null;
	try {
		await cancelTeamInvitation(teamId, invitationId);
		teamInvitations = teamInvitations.filter((i) => i.id !== invitationId);
	} catch (e) {
		inviteError = e instanceof Error ? e.message : 'Failed to cancel invitation';
	}
}

$effect(() => {
	if (!isAuthenticated()) {
		loading = false;
		unauthorized = true;
		return;
	}
	fetchData();
});
</script>

<svelte:head>
	<title>{team?.name ?? 'Team'} - opensession.io</title>
</svelte:head>

{#if loading}
	<div class="py-8 text-center text-xs text-text-muted">Loading team...</div>
{:else if unauthorized}
	<div class="mx-auto max-w-2xl py-6">
		<AuthGuideCard
			title="Team pages require sign in"
			description="Sign in with your account to view team members, invites, and team sessions."
		/>
	</div>
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
					<form onsubmit={(e) => { e.preventDefault(); handleInvite(); }} class="flex flex-wrap items-center gap-2">
						<div class="flex h-7 gap-1">
							<button
								type="button"
								onclick={() => { inviteType = 'email'; }}
								class="px-2 text-xs"
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
								class="px-2 text-xs"
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
								class="h-7 border border-border bg-bg-primary px-2 text-xs text-text-primary outline-none"
							>
								<option value="github">GitHub</option>
								<option value="gitlab">GitLab</option>
							</select>
						{/if}
						<input
							type="text"
							placeholder={inviteType === 'email' ? 'user@example.com' : 'username'}
							bind:value={inviteTarget}
							class="h-7 min-w-56 flex-1 border border-border bg-bg-primary px-3 text-xs text-text-primary placeholder-text-muted outline-none focus:border-accent"
						/>
						<select
							bind:value={inviteRole}
							class="h-7 border border-border bg-bg-primary px-2 text-xs text-text-primary outline-none"
						>
							<option value="member">member</option>
							<option value="admin">admin</option>
						</select>
						<button
							type="submit"
							disabled={inviting || !inviteTarget.trim()}
							class="h-7 bg-accent px-3 text-xs text-white hover:bg-accent/80 disabled:opacity-50"
						>
							{inviting ? 'Sending...' : 'Invite'}
						</button>
					</form>
					{#if inviteError}
						<p class="mt-1 text-xs text-error">{inviteError}</p>
					{/if}
					{#if inviteSuccess}
						<p class="mt-1 text-xs text-success">
							Invitation created. (No email is sent yet; recipient sees it in Inbox once matched.)
						</p>
					{/if}
					{#if teamInvitationsLoading}
						<p class="mt-2 text-xs text-text-muted">Loading pending invitations...</p>
					{:else if teamInvitations.length > 0}
						<div class="mt-2 space-y-1">
							{#each teamInvitations as inv (inv.id)}
								<div class="flex items-center justify-between gap-2 border border-border px-2 py-1">
									<div class="min-w-0 text-xs text-text-secondary">
										<p class="text-text-primary">
											{inv.email ?? `${inv.oauth_provider}:${inv.oauth_provider_username}`}
										</p>
										<p>role: {inv.role} · created {inv.created_at}</p>
									</div>
									<button
										onclick={() => handleCancelInvitation(inv.id)}
										class="px-2 py-0.5 text-xs text-error hover:underline"
									>
										Cancel
									</button>
								</div>
							{/each}
						</div>
					{/if}
				</div>
			{/if}

			<!-- One-time Invite Keys (team admin only) -->
			{#if isTeamAdmin}
				<div class="border-b border-border px-3 py-2">
					<h3 class="mb-2 text-sm font-medium text-text-primary">Invite Keys (single-use)</h3>
					<p class="mb-2 text-xs text-text-muted">
						Each key can be used once, then it becomes invalid automatically.
					</p>
					<div class="mb-2 flex flex-wrap items-end gap-2">
						<div>
							<p class="mb-1 text-[11px] text-text-muted">Role</p>
							<select
								bind:value={keyRole}
								class="h-7 border border-border bg-bg-primary px-2 text-xs text-text-primary outline-none"
							>
								<option value="member">member</option>
								<option value="admin">admin</option>
							</select>
						</div>
						<div>
							<p class="mb-1 text-[11px] text-text-muted">Expires (days)</p>
							<input
								type="number"
								min="1"
								max="30"
								bind:value={keyDays}
								class="h-7 w-28 border border-border bg-bg-primary px-2 text-xs text-text-primary outline-none"
							/>
						</div>
						<button
							onclick={handleCreateInviteKey}
							disabled={creatingKey}
							class="h-7 bg-accent px-3 text-xs text-white hover:bg-accent/80 disabled:opacity-50"
						>
							{creatingKey ? 'Creating...' : 'Create key'}
						</button>
					</div>
					<p class="mb-2 text-xs text-text-muted">
						This section does not use email/OAuth target fields. It generates a single-use key to share directly.
					</p>
					<div class="mb-2 flex items-center gap-2">
						<button
							onclick={() => (showInactiveKeys = !showInactiveKeys)}
							class="bg-bg-hover px-2 py-0.5 text-xs text-text-secondary hover:text-text-primary"
						>
							{showInactiveKeys ? 'Hide inactive' : 'Show inactive'}
						</button>
						<span class="text-xs text-text-muted">(used/revoked keys)</span>
					</div>

					{#if createdInviteKey}
						<div class="mb-2 border border-accent/30 bg-bg-hover p-2">
							<p class="mb-1 text-xs text-text-primary">Copy now (shown once):</p>
							<code class="block break-all text-xs text-accent">{createdInviteKey}</code>
						</div>
					{/if}

					{#if keyError}
						<p class="mb-1 text-xs text-error">{keyError}</p>
					{/if}

					{#if keysLoading}
						<p class="text-xs text-text-muted">Loading keys...</p>
					{:else if inviteKeys.filter((key) => showInactiveKeys || (!key.used_at && !key.revoked_at)).length === 0}
						<p class="text-xs text-text-muted">No active/recent keys.</p>
					{:else}
						<div class="space-y-1">
							{#each inviteKeys
								.filter((key) => showInactiveKeys || (!key.used_at && !key.revoked_at))
								.slice(0, 12) as key (key.id)}
								<div class="flex items-center justify-between gap-2 border border-border px-2 py-1">
									<div class="min-w-0">
										<p class="text-xs text-text-primary">
											{key.role} · by @{key.created_by_nickname}
										</p>
										<p class="text-xs text-text-muted">
											expires {key.expires_at}
											{#if key.used_at}
												 · used
											{:else if key.revoked_at}
												 · revoked
											{:else}
												 · active
											{/if}
										</p>
									</div>
									{#if !key.used_at && !key.revoked_at}
										<button
											onclick={() => handleRevokeInviteKey(key.id)}
											class="px-2 py-0.5 text-xs text-error hover:underline"
										>
											Revoke
										</button>
									{/if}
								</div>
							{/each}
						</div>
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
