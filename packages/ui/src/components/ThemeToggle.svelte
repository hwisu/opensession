<script lang="ts">
import { onMount } from 'svelte';
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
	class="flex h-8 w-8 items-center justify-center border border-border bg-bg-secondary text-sm text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary"
	title={isDark ? 'Switch to light mode' : 'Switch to dark mode'}
	aria-label={isDark ? 'Switch to light mode' : 'Switch to dark mode'}
>
	{#if isDark}
		<span>{@html sunIcon}</span>
	{:else}
		<span>{@html moonIcon}</span>
	{/if}
</button>
