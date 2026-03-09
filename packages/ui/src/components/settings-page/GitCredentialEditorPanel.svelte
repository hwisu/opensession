<script lang="ts">
type Props = {
	credentialLabel: string;
	credentialHost: string;
	credentialPathPrefix: string;
	credentialHeaderName: string;
	credentialHeaderValue: string;
	creatingCredential: boolean;
	onSaveCredential: () => void;
};

let {
	credentialLabel = $bindable(),
	credentialHost = $bindable(),
	credentialPathPrefix = $bindable(),
	credentialHeaderName = $bindable(),
	credentialHeaderValue = $bindable(),
	creatingCredential,
	onSaveCredential,
}: Props = $props();
</script>

<div class="mt-4 space-y-3">
	<div class="grid gap-2 sm:grid-cols-2">
		<input
			data-testid="git-credential-label"
			type="text"
			placeholder="Label"
			bind:value={credentialLabel}
			class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
		/>
		<input
			data-testid="git-credential-host"
			type="text"
			placeholder="Host (e.g. gitlab.internal.example.com)"
			bind:value={credentialHost}
			class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
		/>
		<input
			data-testid="git-credential-path-prefix"
			type="text"
			placeholder="Path prefix (optional, e.g. group/subgroup)"
			bind:value={credentialPathPrefix}
			class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
		/>
		<input
			data-testid="git-credential-header-name"
			type="text"
			placeholder="Header name"
			bind:value={credentialHeaderName}
			class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
		/>
		<input
			data-testid="git-credential-header-value"
			type="password"
			placeholder="Header value (secret)"
			bind:value={credentialHeaderValue}
			class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary sm:col-span-2"
		/>
	</div>
	<div class="flex justify-end">
		<button
			type="button"
			data-testid="git-credential-save"
			onclick={onSaveCredential}
			disabled={creatingCredential}
			class="bg-accent px-3 py-2 text-xs font-semibold text-white hover:bg-accent/85 disabled:opacity-60"
		>
			{creatingCredential ? 'Saving...' : 'Save credential'}
		</button>
	</div>
</div>
