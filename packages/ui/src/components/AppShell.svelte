<script lang="ts">
import type { Snippet } from 'svelte';
import { tick } from 'svelte';
import {
	ApiError,
	authLogout,
	getApiCapabilities,
	getSettings,
	isAuthenticated,
	verifyAuth,
} from '../api';
import type { UserSettings } from '../types';
import ThemeToggle from './ThemeToggle.svelte';

type PaletteCommand = {
	id: string;
	label: string;
	description: string;
	keywords: string[];
	run: () => void;
};

const {
	currentPath,
	children,
	onNavigate = (path: string) => {
		window.location.assign(path);
	},
}: {
	currentPath: string;
	children: Snippet;
	onNavigate?: (path: string) => void;
} = $props();

let user = $state<UserSettings | null>(null);
let paletteOpen = $state(false);
let paletteQuery = $state('');
let paletteSelectionIndex = $state(0);
let paletteInput: HTMLInputElement | undefined = $state();
let accountMenuOpen = $state(false);
let accountMenuRoot: HTMLDivElement | undefined = $state();
let authEnabled = $state(false);
let hasLocalAuth = $state(false);

const isSessionDetail = $derived(currentPath.startsWith('/session/'));
const isSessionList = $derived(currentPath === '/sessions');

function trimNonEmpty(value: string | null | undefined): string | null {
	if (typeof value !== 'string') return null;
	const trimmed = value.trim();
	return trimmed.length > 0 ? trimmed : null;
}

function normalizeAtHandle(value: string | null | undefined): string | null {
	const trimmed = trimNonEmpty(value);
	if (!trimmed) return null;
	const withoutAt = trimmed.replace(/^@+/, '');
	return withoutAt.length > 0 ? `@${withoutAt}` : null;
}

function navAccountHandle(currentUser: UserSettings | null): string {
	if (!currentUser) return 'account';

	const githubHandle = currentUser.oauth_providers
		?.find((provider) => provider.provider.toLowerCase() === 'github')
		?.provider_username;
	const preferredGithub = normalizeAtHandle(githubHandle);
	if (preferredGithub) return preferredGithub;

	return trimNonEmpty(currentUser.nickname) ?? 'account';
}

function shortDate(iso: string | null | undefined): string {
	if (!iso) return '-';
	const parsed = new Date(iso);
	if (Number.isNaN(parsed.getTime())) return '-';
	return parsed.toLocaleDateString();
}

function linkedProvidersLabel(currentUser: UserSettings | null): string {
	if (!currentUser || !currentUser.oauth_providers || currentUser.oauth_providers.length === 0) {
		return 'none';
	}
	return currentUser.oauth_providers.map((provider) => provider.display_name).join(', ');
}

function splitShortcutHint(hint: string): { combo: string; description: string } {
	const firstSpace = hint.indexOf(' ');
	if (firstSpace < 0) return { combo: hint, description: '' };
	return {
		combo: hint.slice(0, firstSpace),
		description: hint.slice(firstSpace + 1),
	};
}

const navLinks = $derived.by(() => {
	const links: Array<{ href: string; label: string }> = [{ href: '/sessions', label: 'Sessions' }];
	links.push({ href: '/docs', label: 'Docs' });
	return links;
});

$effect(() => {
	let cancelled = false;
	getApiCapabilities()
		.then((capabilities) => {
			if (cancelled) return;
			authEnabled = capabilities.auth_enabled;
		})
		.catch(() => {
			if (cancelled) return;
			authEnabled = false;
		});

	return () => {
		cancelled = true;
	};
});

const shortcutHints = $derived.by(() => {
	if (isSessionDetail) {
		return ['Cmd/Ctrl+K palette', 'j/k scroll', '1-5 filters', '/ search', 'n/p match', 'Esc back'];
	}
	if (isSessionList) {
		return [
			'Cmd/Ctrl+K palette',
			'j/k navigate',
			'Enter open',
			'/ search',
			't tool',
			'o order',
			'r range',
		];
	}
	return ['Cmd/Ctrl+K palette', 'Esc back'];
});

$effect(() => {
	void currentPath;
	if (!authEnabled) {
		user = null;
		hasLocalAuth = false;
		accountMenuOpen = false;
		return;
	}

	if (!isAuthenticated()) {
		user = null;
		hasLocalAuth = false;
		accountMenuOpen = false;
		return;
	}

	let cancelled = false;
	verifyAuth()
		.then(async (ok) => {
			if (!ok || cancelled) {
				user = null;
				hasLocalAuth = false;
				accountMenuOpen = false;
				return;
			}
			try {
				const settings = await getSettings();
				if (!cancelled) {
					user = settings;
					hasLocalAuth = true;
				}
			} catch (e) {
				if (cancelled) return;
				user = null;
				hasLocalAuth = false;
				accountMenuOpen = false;
				if (e instanceof ApiError && (e.status === 401 || e.status === 403)) {
					await authLogout();
				}
			}
		})
		.catch(() => {
			if (!cancelled) {
				user = null;
				hasLocalAuth = false;
				accountMenuOpen = false;
			}
		});

	return () => {
		cancelled = true;
	};
});

$effect(() => {
	void currentPath;
	accountMenuOpen = false;
});

function createPaletteCommand(
	id: string,
	label: string,
	description: string,
	keywords: string[],
	run: () => void,
): PaletteCommand {
	return { id, label, description, keywords, run };
}

function dispatchFocusSearch() {
	window.dispatchEvent(new CustomEvent('opensession:focus-search'));
}

const allPaletteCommands = $derived.by(() => {
	const commands: PaletteCommand[] = [
		createPaletteCommand(
			'go-sessions',
			'Go to Sessions',
			'Open the main session list',
			['sessions', 'home', 'list', '/sessions'],
			() => onNavigate('/sessions'),
		),
		createPaletteCommand(
			'go-docs',
			'Go to Docs',
			'Open product and API documentation',
			['docs', 'documentation', 'guide'],
			() => onNavigate('/docs'),
		),
	];

	if (!hasLocalAuth) {
		commands.push(
			createPaletteCommand(
				'go-login',
				'Go to Login',
				'Sign in to your account',
				['login', 'auth', 'signin'],
				() => onNavigate('/login'),
			),
		);
	}
	if (hasLocalAuth) {
		commands.push(
			createPaletteCommand(
				'go-settings',
				'Go to Settings',
				'Open personal settings and API key controls',
				['settings', 'account', 'profile', 'api key'],
				() => onNavigate('/settings'),
			),
		);
	}

	if (isSessionList) {
		commands.push(
			createPaletteCommand(
				'focus-list-search',
				'Focus session search',
				'Move cursor to list search input',
				['search', 'find', 'session list'],
				dispatchFocusSearch,
			),
		);
	}

	if (isSessionDetail) {
		commands.push(
			createPaletteCommand(
				'focus-detail-search',
				'Focus in-session search',
				'Move cursor to timeline search input',
				['search', 'find', 'timeline', 'session detail'],
				dispatchFocusSearch,
			),
		);
	}

	return commands;
});

const normalizedPaletteQuery = $derived(paletteQuery.trim().toLowerCase());
const visiblePaletteCommands = $derived.by(() => {
	if (!normalizedPaletteQuery) return allPaletteCommands;

	return allPaletteCommands.filter((command) => {
		const haystack = [command.label, command.description, ...command.keywords]
			.join(' ')
			.toLowerCase();
		return haystack.includes(normalizedPaletteQuery);
	});
});

$effect(() => {
	void normalizedPaletteQuery;
	paletteSelectionIndex = 0;
});

$effect(() => {
	const max = visiblePaletteCommands.length - 1;
	if (max < 0) {
		paletteSelectionIndex = 0;
		return;
	}
	if (paletteSelectionIndex > max) {
		paletteSelectionIndex = max;
	}
});

function isLinkActive(href: string): boolean {
	if (href === '/sessions') return currentPath === '/sessions' || currentPath.startsWith('/session/');
	return currentPath === href;
}

function isPaletteShortcut(e: KeyboardEvent): boolean {
	return e.key.toLowerCase() === 'k' && (e.metaKey || e.ctrlKey);
}

async function openPalette() {
	paletteOpen = true;
	paletteQuery = '';
	paletteSelectionIndex = 0;
	await tick();
	paletteInput?.focus();
}

function closePalette() {
	paletteOpen = false;
	paletteQuery = '';
}

function closeAccountMenu() {
	accountMenuOpen = false;
}

function toggleAccountMenu() {
	accountMenuOpen = !accountMenuOpen;
}

function handleWindowPointerDown(e: MouseEvent) {
	if (!accountMenuOpen) return;
	const target = e.target;
	if (!(target instanceof Node)) return;
	if (accountMenuRoot?.contains(target)) return;
	closeAccountMenu();
}

function executePaletteCommand(command: PaletteCommand | undefined) {
	if (!command) return;
	closePalette();
	command.run();
}

function movePaletteSelection(direction: 1 | -1) {
	const len = visiblePaletteCommands.length;
	if (len === 0) return;
	paletteSelectionIndex = (paletteSelectionIndex + direction + len) % len;
}

function handlePaletteInputKeydown(e: KeyboardEvent) {
	if (e.key === 'ArrowDown') {
		e.preventDefault();
		movePaletteSelection(1);
		return;
	}
	if (e.key === 'ArrowUp') {
		e.preventDefault();
		movePaletteSelection(-1);
		return;
	}
	if (e.key === 'Enter') {
		e.preventDefault();
		executePaletteCommand(visiblePaletteCommands[paletteSelectionIndex]);
		return;
	}
	if (e.key === 'Escape') {
		e.preventDefault();
		closePalette();
	}
}

async function handleSignOut() {
	closeAccountMenu();
	await authLogout();
	user = null;
	hasLocalAuth = false;
	onNavigate('/sessions');
}

function handleAccountMenuNavigate(path: string) {
	closeAccountMenu();
	onNavigate(path);
}

function handleGlobalKey(e: KeyboardEvent) {
	if (isPaletteShortcut(e)) {
		e.preventDefault();
		if (paletteOpen) {
			closePalette();
		} else {
			void openPalette();
		}
		return;
	}

	if (accountMenuOpen && e.key === 'Escape') {
		e.preventDefault();
		closeAccountMenu();
		return;
	}

	if (paletteOpen) {
		if (e.key === 'Escape') {
			e.preventDefault();
			closePalette();
		}
		return;
	}

	if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
	if (e.key === 'Escape' && !isSessionList) {
		e.preventDefault();
		history.back();
	}
}
</script>

<svelte:window onkeydown={handleGlobalKey} onmousedown={handleWindowPointerDown} />

<div class="grid h-[100dvh] max-w-[100vw] grid-rows-[auto_1fr_auto] overflow-hidden bg-bg-primary text-text-primary">
	<nav class="relative z-30 flex min-w-0 flex-wrap items-center justify-between gap-2 border-b border-border bg-bg-secondary px-2 py-2 sm:px-4">
		<div class="flex items-center gap-1">
			<a href="/" class="text-sm font-bold tracking-tight text-text-primary sm:text-base">
				opensession<span class="text-accent">.io</span>
			</a>
			<ThemeToggle />
		</div>
		<div class="flex min-w-0 flex-1 items-center justify-end gap-0.5 pb-1 sm:pb-0">
			<div class="flex min-w-0 flex-1 flex-wrap items-center justify-end gap-0.5 overflow-hidden">
				{#each navLinks as link}
					<a
						href={link.href}
						class="px-1.5 py-1 text-xs text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary sm:px-3 sm:text-sm"
						class:text-accent={isLinkActive(link.href)}
					>
						{link.label}
					</a>
				{/each}
			</div>

			{#if hasLocalAuth}
				<div class="relative shrink-0" bind:this={accountMenuRoot}>
					<button
						type="button"
						data-testid="account-menu-trigger"
						aria-expanded={accountMenuOpen}
						aria-haspopup="menu"
						onclick={toggleAccountMenu}
						class="px-1.5 py-1 text-xs text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary sm:px-3 sm:text-sm"
					>
						[{navAccountHandle(user)}]
					</button>

						{#if accountMenuOpen}
							<div
								role="menu"
								aria-label="Account menu"
								data-testid="account-menu"
								class="absolute right-0 z-40 mt-1 w-[min(18rem,calc(100vw-1rem))] border border-border bg-bg-primary shadow-2xl"
							>
							<div class="border-b border-border px-3 py-2">
								<p class="text-[11px] uppercase tracking-[0.1em] text-text-muted">Account</p>
								<p class="mt-1 text-sm font-medium text-text-primary">{user?.nickname}</p>
								<p class="text-xs text-text-secondary">{user?.email ?? 'email not linked'}</p>
							</div>

							<div class="border-b border-border px-3 py-2 text-xs text-text-secondary">
								<p>User ID: <span class="text-text-primary">{user?.user_id}</span></p>
								<p>Joined: <span class="text-text-primary">{shortDate(user?.created_at)}</span></p>
								<p>Providers: <span class="text-text-primary">{linkedProvidersLabel(user)}</span></p>
							</div>

								<div class="border-b border-border px-2 py-1">
									<button
										type="button"
										class="block w-full px-2 py-1 text-left text-xs text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary"
										onclick={() => handleAccountMenuNavigate('/settings')}
									>
										Settings
									</button>
									<button
										type="button"
										class="block w-full px-2 py-1 text-left text-xs text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary"
									onclick={() => handleAccountMenuNavigate('/sessions')}
								>
									Session Home
								</button>
								<button
									type="button"
									class="block w-full px-2 py-1 text-left text-xs text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary"
									onclick={() => handleAccountMenuNavigate('/docs')}
								>
									Docs
								</button>
							</div>

							<div class="px-2 py-1">
								<button
									type="button"
									data-testid="account-menu-logout"
									onclick={handleSignOut}
									class="block w-full px-2 py-1 text-left text-xs text-error transition-colors hover:bg-error/10"
								>
									Logout
								</button>
							</div>
						</div>
					{/if}
				</div>
			{:else}
				<a
					href="/login"
					class="px-1.5 py-1 text-xs text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary sm:px-3 sm:text-sm"
				>
					Login
				</a>
			{/if}
		</div>
	</nav>

	<main class="relative z-0 overflow-y-auto overscroll-contain px-2 py-3 sm:px-4">
		{@render children()}
	</main>

	<footer
		data-testid="shortcut-footer"
		class="shrink-0 flex items-center gap-2 border-t border-border bg-bg-secondary px-2 py-1 text-[11px] text-text-muted sm:gap-3 sm:px-4 sm:text-xs"
	>
		<span class="font-semibold tracking-[0.06em] text-accent/90">
			Shortcuts
		</span>
		<span class="sm:hidden inline-flex items-center gap-1 text-text-secondary">
			<kbd class="font-mono text-[10px] font-semibold text-accent">
				Cmd/Ctrl+K
			</kbd>
		</span>
		{#if isSessionList}
			<span data-testid="tor-footer-hint" class="inline-flex items-center gap-1 text-text-secondary">
				<kbd class="font-mono text-[10px] font-semibold text-accent">t</kbd>
				<span>tool</span>
				<kbd class="font-mono text-[10px] font-semibold text-accent">o</kbd>
				<span>order</span>
				<kbd class="font-mono text-[10px] font-semibold text-accent">r</kbd>
				<span>range</span>
			</span>
		{/if}
		{#each shortcutHints as hint}
			{@const parsedHint = splitShortcutHint(hint)}
			<span class="hidden sm:inline-flex items-center gap-1 text-text-secondary">
				<kbd class="font-mono text-[10px] font-semibold text-accent">
					{parsedHint.combo}
				</kbd>
				{#if parsedHint.description}
					<span>{parsedHint.description}</span>
				{/if}
			</span>
		{/each}
		<span class="ml-auto">opensession.io</span>
	</footer>
</div>

{#if paletteOpen}
	<div class="fixed inset-0 z-50 p-4">
		<button
			type="button"
			class="absolute inset-0 bg-black/60"
			aria-label="Close command palette"
			onclick={closePalette}
		></button>
		<div
			role="dialog"
			aria-modal="true"
			aria-label="Command Palette"
			tabindex="-1"
			data-testid="command-palette"
			class="relative mx-auto mt-14 w-full max-w-2xl border border-border bg-bg-primary shadow-2xl"
		>
			<div class="border-b border-border px-3 py-2">
				<input
					bind:this={paletteInput}
					bind:value={paletteQuery}
					onkeydown={handlePaletteInputKeydown}
					data-testid="command-palette-input"
					type="text"
					placeholder="Type a command or page name..."
					class="w-full border border-border bg-bg-secondary px-2 py-1 text-sm text-text-primary outline-none focus:border-accent"
				/>
			</div>
			<div class="max-h-[24rem] overflow-y-auto">
				{#if visiblePaletteCommands.length === 0}
					<p class="px-3 py-3 text-xs text-text-muted">No commands matched your query.</p>
				{:else}
					{#each visiblePaletteCommands as command, idx (command.id)}
						<button
							type="button"
							class="flex w-full items-start justify-between gap-3 border-b border-border/60 px-3 py-2 text-left"
							class:bg-bg-secondary={idx === paletteSelectionIndex}
							class:hover:bg-bg-secondary={idx !== paletteSelectionIndex}
							onclick={() => executePaletteCommand(command)}
						>
							<span>
								<span class="block text-sm text-text-primary">{command.label}</span>
								<span class="block text-xs text-text-muted">{command.description}</span>
							</span>
							<span class="text-[11px] text-text-muted">Enter</span>
						</button>
					{/each}
				{/if}
			</div>
			<div class="flex items-center justify-between border-t border-border px-3 py-1.5 text-[11px] text-text-muted">
				<span>{visiblePaletteCommands.length} command{visiblePaletteCommands.length === 1 ? '' : 's'}</span>
				<span>Up/Down navigate · Enter run · Esc close</span>
			</div>
		</div>
	</div>
{/if}
