<script lang="ts">
import { tick } from 'svelte';
import type { Snippet } from 'svelte';
import { getSettings, isAuthenticated, listInvitations } from '../api';
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
	appProfile = 'docker',
	onNavigate = (path: string) => {
		window.location.assign(path);
	},
}: {
	currentPath: string;
	children: Snippet;
	appProfile?: 'docker' | 'worker';
	onNavigate?: (path: string) => void;
} = $props();

let user = $state<UserSettings | null>(null);
let inboxCount = $state(0);
let lastSettingsFetchAt = $state(0);
let lastInboxFetchAt = $state(0);
let paletteOpen = $state(false);
let paletteQuery = $state('');
let paletteSelectionIndex = $state(0);
let paletteInput: HTMLInputElement | undefined = $state();

const SETTINGS_REFRESH_INTERVAL_MS = 30_000;
const INBOX_REFRESH_INTERVAL_MS = 30_000;
const teamFeaturesEnabled = $derived(appProfile === 'docker');
const isSessionDetail = $derived(currentPath.startsWith('/session/'));
const isSessionList = $derived(currentPath === '/');

const navLinks = $derived.by(() => {
	const links: Array<{ href: string; label: string }> = [{ href: '/', label: 'Sessions' }];
	if (user) {
		if (teamFeaturesEnabled) {
			links.push({ href: '/teams', label: 'Teams' });
			links.push({ href: '/invitations', label: 'Inbox' });
		}
		links.push({ href: '/upload', label: 'Upload' });
	}
	links.push({ href: '/dx', label: 'DX' });
	links.push({ href: '/docs', label: 'Docs' });
	return links;
});

const shortcutHints = $derived.by(() => {
	if (isSessionDetail) {
		return ['Cmd/Ctrl+K palette', 'j/k scroll', '1-5 filters', '/ search', 'n/p match', 'Esc back'];
	}
	if (isSessionList) {
		return ['Cmd/Ctrl+K palette', 'j/k navigate', 'Enter open', '/ search', 't/o/r cycle', 'l layout'];
	}
	return ['Cmd/Ctrl+K palette', 'Esc back'];
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
			['sessions', 'home', 'list', '/'],
			() => onNavigate('/'),
		),
		createPaletteCommand(
			'go-docs',
			'Go to Docs',
			'Open product and API documentation',
			['docs', 'documentation', 'guide'],
			() => onNavigate('/docs'),
		),
		createPaletteCommand(
			'go-dx',
			'Go to DX Lab',
			'Open parser playground and conformance dashboard',
			['dx', 'playground', 'conformance', 'parser'],
			() => onNavigate('/dx'),
		),
	];

	if (user) {
		commands.push(
			createPaletteCommand(
				'go-upload',
				'Go to Upload',
				'Upload a HAIL session file',
				['upload', 'ingest', 'jsonl'],
				() => onNavigate('/upload'),
			),
			createPaletteCommand(
				'go-settings',
				'Go to Settings',
				'Open user settings and API key',
				['settings', 'profile', 'api key'],
				() => onNavigate('/settings'),
			),
		);
		if (teamFeaturesEnabled) {
			commands.push(
				createPaletteCommand(
					'go-teams',
					'Go to Teams',
					'Open team workspace list',
					['teams', 'collaboration', 'members'],
					() => onNavigate('/teams'),
				),
				createPaletteCommand(
					'go-inbox',
					'Go to Inbox',
					'Review team invitations',
					['inbox', 'invitation', 'invite'],
					() => onNavigate('/invitations'),
				),
			);
		}
	} else {
		commands.push(
			createPaletteCommand(
				'go-login',
				'Go to Login',
				'Sign in to access uploads and settings',
				['login', 'signin', 'auth'],
				() => onNavigate('/login'),
			),
			createPaletteCommand(
				'go-register',
				'Go to Register',
				'Create a new account',
				['register', 'signup', 'auth'],
				() => onNavigate('/register'),
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
		const haystack = [
			command.label,
			command.description,
			...command.keywords,
		]
			.join(' ')
			.toLowerCase();
		return haystack.includes(normalizedPaletteQuery);
	});
});

$effect(() => {
	// Re-check auth/inbox on navigation with a short throttle window.
	void currentPath;
	const now = Date.now();
	const hasStoredAuth = isAuthenticated();
	const shouldCheckSettings = hasStoredAuth || user !== null;

	if (!shouldCheckSettings) {
		user = null;
		inboxCount = 0;
		return;
	}

	if (now - lastSettingsFetchAt >= SETTINGS_REFRESH_INTERVAL_MS) {
		lastSettingsFetchAt = now;
		getSettings()
			.then((u) => {
				user = u;
			})
			.catch(() => {
				user = null;
			});
	}

	const hasAuthContext = !!user || hasStoredAuth;
	if (!hasAuthContext || !teamFeaturesEnabled) {
		inboxCount = 0;
		return;
	}

	if (now - lastInboxFetchAt >= INBOX_REFRESH_INTERVAL_MS) {
		lastInboxFetchAt = now;
		listInvitations()
			.then((resp) => {
				inboxCount = resp.invitations.length;
			})
			.catch(() => {
				inboxCount = 0;
			});
	}
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
	if (href === '/') return currentPath === '/' || currentPath.startsWith('/session/');
	if (href === '/teams') return currentPath === '/teams' || currentPath.startsWith('/teams/');
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
	<footer
		data-testid="shortcut-footer"
		class="shrink-0 flex items-center gap-4 border-t border-border bg-bg-secondary px-4 py-1 text-xs text-text-muted"
	>
		<span class="font-medium text-text-secondary">Shortcuts</span>
		{#each shortcutHints as hint}
			<span>{hint}</span>
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
