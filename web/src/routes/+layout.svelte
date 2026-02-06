<script lang="ts">
	import '../app.css';
	import type { Snippet } from 'svelte';
	import { getSettings } from '$lib/api';
	import type { UserSettings } from '$lib/types';

	let { children }: { children: Snippet } = $props();

	let user = $state<UserSettings | null>(null);

	const navLinks = [
		{ href: '/', label: 'Home' },
		{ href: '/groups', label: 'Groups' },
		{ href: '/upload', label: 'Upload' }
	];

	$effect(() => {
		getSettings().then((u) => { user = u; }).catch(() => { user = null; });
	});
</script>

<div class="flex min-h-screen flex-col bg-bg-primary text-text-primary">
	<nav class="sticky top-0 z-50 border-b border-border bg-bg-secondary/80 backdrop-blur-sm">
		<div class="mx-auto flex h-14 max-w-6xl items-center justify-between px-4">
			<a href="/" class="text-lg font-bold tracking-tight text-white">
				opensession<span class="text-accent">.io</span>
			</a>
			<div class="flex items-center gap-1">
				{#each navLinks as link}
					<a
						href={link.href}
						class="rounded-md px-3 py-1.5 text-sm text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary"
					>
						{link.label}
					</a>
				{/each}
				{#if user}
					<a
						href="/settings"
						class="ml-1 flex items-center rounded-full transition-opacity hover:opacity-80"
						title={user.nickname}
					>
						{#if user.avatar_url}
							<img
								src={user.avatar_url}
								alt={user.nickname}
								class="h-7 w-7 rounded-full ring-2 ring-border"
							/>
						{:else}
							<div class="flex h-7 w-7 items-center justify-center rounded-full bg-bg-hover text-xs font-bold text-text-secondary ring-2 ring-border">
								{user.nickname[0].toUpperCase()}
							</div>
						{/if}
					</a>
				{:else}
					<a
						href="/settings"
						class="rounded-md px-3 py-1.5 text-sm text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary"
					>
						Sign In
					</a>
				{/if}
			</div>
		</div>
	</nav>

	<main class="mx-auto w-full max-w-6xl flex-1 px-4 py-6">
		{@render children()}
	</main>

	<footer class="border-t border-border py-6 text-center text-sm text-text-muted">
		<div class="mx-auto max-w-6xl px-4">
			<p>opensession.io -- HAIL (Human AI Interaction Log) for AI sessions</p>
			<div class="mt-2 flex items-center justify-center gap-4">
				<a href="https://github.com/opensession" class="hover:text-text-secondary">GitHub</a>
				<a href="/upload" class="hover:text-text-secondary">Upload</a>
				<a href="/settings" class="hover:text-text-secondary">Settings</a>
			</div>
		</div>
	</footer>
</div>
