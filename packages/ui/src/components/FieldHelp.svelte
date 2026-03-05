<script lang="ts">
const {
	label,
	help,
	testId,
	inline = false,
}: {
	label: string;
	help: string;
	testId?: string;
	inline?: boolean;
} = $props();
</script>

{#if inline}
	<span class="inline-flex items-center gap-1">
		<span>{label}</span>
		<span
			class="inline-flex h-4 w-4 cursor-help items-center justify-center border border-border/70 text-[10px] leading-none text-text-muted"
			title={help}
			aria-label={`${label} help: ${help}`}
			data-testid={testId}
			>?</span
		>
	</span>
{:else}
	<span class="mb-1 flex items-center gap-1">
		<span>{label}</span>
		<span
			class="inline-flex h-4 w-4 cursor-help items-center justify-center border border-border/70 text-[10px] leading-none text-text-muted"
			title={help}
			aria-label={`${label} help: ${help}`}
			data-testid={testId}
			>?</span
		>
	</span>
{/if}
