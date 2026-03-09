<script lang="ts">
const {
	onNavigateToLogin,
}: {
	onNavigateToLogin: () => void;
} = $props();
</script>

<section
	id="settings-section-profile"
	data-testid="settings-require-auth"
	class="scroll-mt-24 border border-border bg-bg-secondary px-4 py-6 text-sm text-text-secondary xl:max-w-3xl"
>
	<p class="text-text-primary">Sign in is required to view personal settings.</p>
	<div class="mt-4">
		<button
			type="button"
			onclick={onNavigateToLogin}
			class="bg-accent px-3 py-2 text-xs font-semibold text-white hover:bg-accent/85"
		>
			Go to login
		</button>
	</div>
</section>
