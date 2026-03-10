<script lang="ts">
import { onMount } from 'svelte';
import { appLocale, translate } from '../i18n';
import { moonIcon, sunIcon } from './icons';

let isDark = $state(true);

onMount(() => {
	const stored = localStorage.getItem('theme');
	if (stored === 'light') {
		isDark = false;
		document.documentElement.classList.add('light');
	} else {
		document.documentElement.classList.remove('light');
	}
});

function toggle() {
	isDark = !isDark;
	document.documentElement.classList.toggle('light', !isDark);
	localStorage.setItem('theme', isDark ? 'dark' : 'light');
}
</script>

<button
	onclick={toggle}
	class="flex items-center justify-center text-sm text-text-secondary transition-colors hover:text-text-primary"
	title={isDark ? translate($appLocale, 'theme.switchLight') : translate($appLocale, 'theme.switchDark')}
	aria-label={isDark ? translate($appLocale, 'theme.switchLight') : translate($appLocale, 'theme.switchDark')}
>
	{#if isDark}
		<span>{@html sunIcon}</span>
	{:else}
		<span>{@html moonIcon}</span>
	{/if}
</button>
