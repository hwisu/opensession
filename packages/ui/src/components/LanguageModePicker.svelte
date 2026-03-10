<script lang="ts">
import {
	appLocale,
	languageMode,
	setLanguagePreference,
	translate,
} from '../i18n';

const {
	compact = false,
}: {
	compact?: boolean;
} = $props();

const options = [
	{ value: 'system', labelKey: 'language.mode.system', shortKey: 'language.short.system' },
	{ value: 'en', labelKey: 'language.mode.en', shortKey: 'language.short.en' },
	{ value: 'ko', labelKey: 'language.mode.ko', shortKey: 'language.short.ko' },
] as const;
</script>

<div class:items-center={compact} class="flex flex-wrap gap-1">
	{#each options as option}
		<button
			type="button"
			onclick={() => setLanguagePreference(option.value)}
			class={`rounded border px-2 py-1 text-xs transition-colors ${
				$languageMode === option.value
					? 'border-accent bg-accent/10 text-accent'
					: 'border-border bg-bg-secondary text-text-secondary hover:text-text-primary'
			}`}
			aria-label={translate($appLocale, option.labelKey)}
			title={translate($appLocale, option.labelKey)}
		>
			{translate($appLocale, option.shortKey)}
		</button>
	{/each}
</div>

{#if !compact}
	<p class="mt-2 text-xs text-text-secondary">
		{translate($appLocale, 'language.help')}
	</p>
	<p class="mt-1 text-[11px] text-text-muted">
		{translate($appLocale, 'language.current', {
			locale:
				$languageMode === 'system'
					? translate($appLocale, 'language.platform')
					: $languageMode === 'en'
						? translate($appLocale, 'language.mode.en')
						: translate($appLocale, 'language.mode.ko'),
		})}
	</p>
{/if}
