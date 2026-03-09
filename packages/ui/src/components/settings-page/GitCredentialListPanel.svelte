<script lang="ts">
import type { GitCredentialSummary } from '../../types';

const {
	credentials,
	credentialsLoading,
	deletingCredentialId,
	onDeleteCredential,
}: {
	credentials: GitCredentialSummary[];
	credentialsLoading: boolean;
	deletingCredentialId: string | null;
	onDeleteCredential: (id: string) => void;
} = $props();
</script>

<div class="mt-4 border border-border/70">
	<div class="grid grid-cols-[1.1fr_1fr_1fr_auto] gap-2 border-b border-border bg-bg-primary px-3 py-2 text-[11px] uppercase tracking-[0.08em] text-text-muted">
		<span>Label</span>
		<span>Host</span>
		<span>Path Prefix</span>
		<span>Action</span>
	</div>
	{#if credentialsLoading}
		<div class="px-3 py-3 text-xs text-text-muted">Loading credentials...</div>
	{:else if credentials.length === 0}
		<div class="px-3 py-3 text-xs text-text-muted">No manual credentials registered.</div>
	{:else}
		{#each credentials as credential}
			<div class="grid grid-cols-[1.1fr_1fr_1fr_auto] items-center gap-2 border-b border-border/60 px-3 py-2 text-xs">
				<div class="text-text-primary">{credential.label}</div>
				<div class="font-mono text-[11px] text-text-secondary">{credential.host}</div>
				<div class="font-mono text-[11px] text-text-secondary">{credential.path_prefix || '*'}</div>
				<button
					type="button"
					data-testid={'git-credential-delete-' + credential.id}
					disabled={deletingCredentialId === credential.id}
					onclick={() => onDeleteCredential(credential.id)}
					class="border border-border px-2 py-1 text-[11px] text-text-secondary hover:text-text-primary disabled:opacity-60"
				>
					{deletingCredentialId === credential.id ? 'Deleting...' : 'Delete'}
				</button>
			</div>
		{/each}
	{/if}
</div>
