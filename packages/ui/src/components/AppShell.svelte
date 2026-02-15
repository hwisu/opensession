<script lang="ts">
import type { Snippet } from 'svelte';
import { getSettings, isAuthenticated, listInvitations } from '../api';
import type { UserSettings } from '../types';
import ThemeToggle from './ThemeToggle.svelte';

const { currentPath, children }: { currentPath: string; children: Snippet } = $props();

let user = $state<UserSettings | null>(null);
let inboxCount = $state(0);

const navLinks = [
	{ href: '/', label: 'Sessions' },
	{ href: '/teams', label: 'Teams' },
	{ href: '/invitations', label: 'Inbox' },
	{ href: '/docs', label: 'Docs' },
];

$effect(() => {
	// Re-fetch user on every navigation (currentPath change triggers re-run)
	void currentPath;
	getSettings()
		.then((u) => {
			user = u;
		})
		.catch(() => {
			user = null;
		});

	if (isAuthenticated()) {
		listInvitations()
			.then((resp) => {
				inboxCount = resp.invitations.length;
			})
			.catch(() => {
				inboxCount = 0;
			});
	} else {
		inboxCount = 0;
	}
});

const isSessionDetail = $derived(currentPath.startsWith('/session/'));
const isSessionList = $derived(currentPath === '/');

function isLinkActive(href: string): boolean {
	if (href === '/') return currentPath === '/' || currentPath.startsWith('/session/');
	if (href === '/teams') return currentPath === '/teams' || currentPath.startsWith('/teams/');
	return currentPath === href;
}

function handleGlobalKey(e: KeyboardEvent) {
	if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
	if (e.key === 'Escape' && !isSessionList) {
		e.preventDefault();
		history.back();
	}
}
</script>

<svelte:window onkeydown={handleGlobalKey} />

<div class="grid h-screen max-w-[100vw] grid-rows-[auto_1fr_auto] overflow-hidden bg-bg-primary text-text-primary">
	<!-- TopBar -->
	<nav class="flex min-w-0 items-center justify-between border-b border-border bg-bg-secondary px-3 py-2 sm:px-4">
		<div class="flex items-center gap-1">
			<a href="/" class="text-sm font-bold tracking-tight text-text-primary sm:text-base">
				opensession<span class="text-accent">.io</span>
			</a>
			<ThemeToggle />
		</div>
		<div class="flex min-w-0 items-center gap-0.5 sm:gap-1">
			{#each navLinks as link}
				<a
					href={link.href}
					class="px-1.5 py-1 text-sm text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary sm:px-3"
					class:text-accent={isLinkActive(link.href)}
				>
					{link.label}
					{#if link.href === '/invitations' && inboxCount > 0}
						<span class="ml-1 inline-block min-w-[1.25rem] rounded bg-accent px-1 text-center text-[10px] font-semibold text-white">
							{inboxCount}
						</span>
					{/if}
				</a>
			{/each}
			{#if user}
				<a
					href="/settings"
					class="ml-1 flex items-center gap-1 px-2 py-1 text-sm text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary"
					title={user.nickname}
				>
					{#if user.avatar_url}
						<img src={user.avatar_url} alt="{user.nickname} avatar" class="h-5 w-5 rounded-full" />
					{/if}
					[{user.nickname}]
				</a>
			{:else}
				<a
					href="/login"
					class="px-1.5 py-1 text-sm text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary sm:px-3"
				>
					Login
				</a>
			{/if}
		</div>
	</nav>

	<!-- Main Content -->
	<main class="overflow-y-auto px-4 py-3">
		{@render children()}
	</main>

	<!-- StatusBar -->
	<footer class="flex items-center gap-6 border-t border-border bg-bg-secondary px-4 py-1 text-xs text-text-muted">
		{#if isSessionDetail}
			<span>j/k scroll</span>
			<span>1-5 filters</span>
			<span>Esc back</span>
		{:else if isSessionList}
			<span>j/k navigate</span>
			<span>Enter open</span>
			<span>/ search</span>
			<span>l layout</span>
		{:else}
			<span>Esc back</span>
		{/if}
		<span class="ml-auto">opensession.io</span>
	</footer>
</div>
