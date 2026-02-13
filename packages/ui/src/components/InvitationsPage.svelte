<script lang="ts">
import { acceptInvitation, declineInvitation, listInvitations } from '../api';
import type { InvitationResponse } from '../types';

const {
	onNavigate = (_path: string) => {},
}: {
	onNavigate?: (path: string) => void;
} = $props();

let invitations = $state<InvitationResponse[]>([]);
let loading = $state(true);
let error = $state<string | null>(null);
let actionLoading = $state<string | null>(null);

async function fetchInvitations() {
	loading = true;
	error = null;
	try {
		const resp = await listInvitations();
		invitations = resp.invitations;
	} catch (e) {
		error = e instanceof Error ? e.message : 'Failed to load invitations';
	} finally {
		loading = false;
	}
}

async function handleAccept(inv: InvitationResponse) {
	actionLoading = inv.id;
	try {
		const result = await acceptInvitation(inv.id);
		invitations = invitations.filter((i) => i.id !== inv.id);
		onNavigate(`/teams/${result.team_id}`);
	} catch (e) {
		if (e instanceof Error) {
			try {
				const parsed = JSON.parse(e.message);
				error = parsed.error || e.message;
			} catch {
				error = e.message;
			}
		} else {
			error = 'Failed to accept invitation';
		}
	} finally {
		actionLoading = null;
	}
}

async function handleDecline(inv: InvitationResponse) {
	actionLoading = inv.id;
	try {
		await declineInvitation(inv.id);
		invitations = invitations.filter((i) => i.id !== inv.id);
	} catch (e) {
		error = e instanceof Error ? e.message : 'Failed to decline invitation';
	} finally {
		actionLoading = null;
	}
}

$effect(() => {
	fetchInvitations();
});
</script>

<svelte:head>
	<title>Invitations - opensession.io</title>
</svelte:head>

<div class="mx-auto max-w-2xl">
	<h1 class="mb-4 text-lg font-bold text-text-primary">Pending Invitations</h1>

	{#if loading}
		<div class="py-8 text-center text-xs text-text-muted">Loading...</div>
	{:else if error}
		<div class="mb-3 border border-error/30 bg-error/10 px-3 py-2 text-xs text-error">
			{error}
		</div>
	{/if}

	{#if !loading && invitations.length === 0}
		<div class="py-8 text-center text-xs text-text-muted">No pending invitations</div>
	{:else}
		<div class="space-y-2">
			{#each invitations as inv (inv.id)}
				<div class="flex items-center justify-between border border-border bg-bg-secondary p-3">
					<div>
						<p class="text-sm font-medium text-text-primary">
							{inv.team_name}
						</p>
						<p class="text-xs text-text-muted">
							Invited by {inv.invited_by_nickname} as {inv.role}
						</p>
						{#if inv.email}
							<p class="text-xs text-text-muted">via {inv.email}</p>
						{:else if inv.oauth_provider && inv.oauth_provider_username}
							<p class="text-xs text-text-muted">via {inv.oauth_provider} @{inv.oauth_provider_username}</p>
						{/if}
					</div>
					<div class="flex gap-2">
						<button
							onclick={() => handleAccept(inv)}
							disabled={actionLoading === inv.id}
							class="bg-accent px-3 py-1 text-xs text-white hover:bg-accent/80 disabled:opacity-50"
						>
							Accept
						</button>
						<button
							onclick={() => handleDecline(inv)}
							disabled={actionLoading === inv.id}
							class="bg-bg-hover px-3 py-1 text-xs text-text-secondary hover:text-text-primary disabled:opacity-50"
						>
							Decline
						</button>
					</div>
				</div>
			{/each}
		</div>
	{/if}
</div>
