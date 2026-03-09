<script lang="ts">
import { appLocale, translate } from '../i18n';
import LanguageModePicker from './LanguageModePicker.svelte';
</script>

<section class="scroll-mt-24 border border-border bg-bg-secondary p-4 xl:max-w-3xl">
	<h2 class="text-sm font-semibold text-text-primary">
		{translate($appLocale, 'settings.languageSection')}
	</h2>
	<p class="mt-1 text-xs text-text-secondary">
		{translate($appLocale, 'settings.languageDescription')}
	</p>
	<div class="mt-3">
		<LanguageModePicker />
	</div>
</section>
