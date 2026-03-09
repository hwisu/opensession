<script lang="ts">
const {
	issuing,
	issuedApiKey,
	copyMessage,
	onIssueApiKey,
	onCopyApiKey,
}: {
	issuing: boolean;
	issuedApiKey: string | null;
	copyMessage: string | null;
	onIssueApiKey: () => void;
	onCopyApiKey: () => void;
} = $props();
</script>

<section
	id="settings-section-api-key"
	class="scroll-mt-24 border border-border bg-bg-secondary p-4 xl:max-w-3xl"
>
	<div class="flex flex-wrap items-center justify-between gap-3">
		<div>
			<h2 class="text-sm font-semibold text-text-primary">Personal API Key</h2>
			<p class="mt-1 text-xs text-text-secondary">
				Issue a new key for CLI and automation access. Existing active key moves to grace mode.
			</p>
		</div>
		<button
			type="button"
			data-testid="issue-api-key-button"
			onclick={onIssueApiKey}
			disabled={issuing}
			class="bg-accent px-3 py-2 text-xs font-semibold text-white hover:bg-accent/85 disabled:opacity-60"
		>
			{issuing ? 'Issuing...' : issuedApiKey ? 'Regenerate key' : 'Issue key'}
		</button>
	</div>

	{#if issuedApiKey}
		<div class="mt-4 border border-border/80 bg-bg-primary p-3">
			<p class="mb-2 text-xs text-text-muted">Shown once. Save this key now.</p>
			<code data-testid="issued-api-key" class="block break-all font-mono text-xs text-text-primary">
				{issuedApiKey}
			</code>
			<div class="mt-3 flex items-center gap-2">
				<button
					type="button"
					data-testid="copy-api-key"
					onclick={onCopyApiKey}
					class="border border-border px-2 py-1 text-xs text-text-secondary hover:text-text-primary"
				>
					Copy
				</button>
				{#if copyMessage}
					<span class="text-xs text-text-muted">{copyMessage}</span>
				{/if}
			</div>
		</div>
	{/if}
</section>
