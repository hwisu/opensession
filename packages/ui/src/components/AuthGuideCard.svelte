<script lang="ts">
const {
	title = 'Sign in required',
	description = 'Sign in to use this page.',
	primaryLabel = 'Sign In',
	primaryPath = '/login',
	secondaryLabel = 'Register',
	secondaryPath = '/register',
	showBrowseSessions = true,
	onNavigate = (_path: string) => {},
}: {
	title?: string;
	description?: string;
	primaryLabel?: string;
	primaryPath?: string;
	secondaryLabel?: string;
	secondaryPath?: string;
	showBrowseSessions?: boolean;
	onNavigate?: (path: string) => void;
} = $props();
</script>

<div class="border border-border bg-bg-secondary p-4">
	<h2 class="mb-2 text-sm font-medium text-text-primary">{title}</h2>
	<p class="mb-3 text-xs leading-relaxed text-text-secondary">{description}</p>
	<div class="flex flex-wrap gap-2">
		<button
			onclick={() => onNavigate(primaryPath)}
			class="bg-accent px-3 py-1.5 text-xs font-medium text-white hover:bg-accent/80"
		>
			{primaryLabel}
		</button>
		<button
			onclick={() => onNavigate(secondaryPath)}
			class="bg-bg-hover px-3 py-1.5 text-xs text-text-secondary hover:text-text-primary"
		>
			{secondaryLabel}
		</button>
		{#if showBrowseSessions}
			<button
				onclick={() => onNavigate('/')}
				class="bg-bg-hover px-3 py-1.5 text-xs text-text-secondary hover:text-text-primary"
			>
				Browse Sessions
			</button>
		{/if}
	</div>
</div>
