<script lang="ts">
import { appLocale } from '../i18n';

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

let open = $state(false);
function tooltipId(): string {
	return `field-help-${(testId ?? label)
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-|-$/g, '')}`;
}

function showHelp() {
	open = true;
}

function hideHelp() {
	open = false;
}

const isKorean = $derived($appLocale === 'ko');
</script>

{#if inline}
	<span class="inline-flex items-center gap-1">
		<span>{label}</span>
		<span class="relative inline-flex">
			<button
				type="button"
				class="inline-flex h-4 w-4 cursor-help items-center justify-center border border-border/70 bg-bg-primary text-[10px] leading-none text-text-muted transition-colors hover:text-text-primary"
				onmouseenter={showHelp}
				onmouseleave={hideHelp}
				onfocus={showHelp}
				onblur={hideHelp}
				onkeydown={(event) => {
					if (event.key === 'Escape') hideHelp();
				}}
				aria-label={isKorean ? `${label} 도움말` : `${label} help`}
				aria-expanded={open}
				aria-describedby={open ? tooltipId() : undefined}
				data-testid={testId}
				>?</button
			>
			{#if open}
				<span
					id={tooltipId()}
					role="tooltip"
					class="pointer-events-none absolute bottom-full right-0 z-20 mb-1 w-56 rounded border border-border bg-bg-secondary px-2 py-1 text-[11px] leading-relaxed text-text-secondary shadow-xl"
					>{help}</span
				>
			{/if}
		</span>
	</span>
{:else}
	<span class="mb-1 flex items-center gap-1">
		<span>{label}</span>
		<span class="relative inline-flex">
			<button
				type="button"
				class="inline-flex h-4 w-4 cursor-help items-center justify-center border border-border/70 bg-bg-primary text-[10px] leading-none text-text-muted transition-colors hover:text-text-primary"
				onmouseenter={showHelp}
				onmouseleave={hideHelp}
				onfocus={showHelp}
				onblur={hideHelp}
				onkeydown={(event) => {
					if (event.key === 'Escape') hideHelp();
				}}
				aria-label={isKorean ? `${label} 도움말` : `${label} help`}
				aria-expanded={open}
				aria-describedby={open ? tooltipId() : undefined}
				data-testid={testId}
				>?</button
			>
			{#if open}
				<span
					id={tooltipId()}
					role="tooltip"
					class="pointer-events-none absolute bottom-full right-0 z-20 mb-1 w-56 rounded border border-border bg-bg-secondary px-2 py-1 text-[11px] leading-relaxed text-text-secondary shadow-xl"
					>{help}</span
				>
			{/if}
		</span>
	</span>
{/if}
