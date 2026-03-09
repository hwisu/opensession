<script lang="ts">
import type { Snippet } from 'svelte';
import { onMount, tick } from 'svelte';
import {
	authLogout,
	getApiCapabilities,
	getSettings,
	isAuthenticated,
	verifyAuth,
} from '../api';
import { createShellModel, createShellModelState } from '../models/app-shell-model';
import type { UserSettings } from '../types';
import {
	appLocale,
	initializeLocalization,
	refreshLocaleFromPlatform,
	translate,
} from '../i18n';
import LanguageModePicker from './LanguageModePicker.svelte';
import ThemeToggle from './ThemeToggle.svelte';

type DesktopWindow = Window & {
	__TAURI_INTERNALS__?: unknown;
};

type DesktopInvoke = <T = unknown>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

type DesktopRuntimeWindow = DesktopWindow & {
	__TAURI__?: {
		core?: {
			invoke?: DesktopInvoke;
		};
	};
};

type PaletteCommand = {
	id: string;
	label: string;
	description: string;
	keywords: string[];
	run: () => void;
};

const OPENSESSION_GITHUB_URL = 'https://github.com/hwisu/opensession';

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

const shellState = $state(createShellModelState());
let paletteInput: HTMLInputElement | undefined = $state();
let helpDialog: HTMLDivElement | undefined = $state();
let accountMenuRoot: HTMLDivElement | undefined = $state();

const isSessionDetail = $derived(currentPath.startsWith('/session/'));
const isSessionList = $derived(currentPath === '/sessions');
const showLoginLink = $derived(
	!shellState.hasLocalAuth && (!shellState.desktopRuntime || shellState.authEnabled),
);

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

	const githubHandle = currentUser.oauth_providers?.find(
		(provider) => provider.provider.toLowerCase() === 'github',
	)?.provider_username;
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

function linkedProvidersLabel(currentUser: UserSettings | null, emptyLabel: string): string {
	if (!currentUser || !currentUser.oauth_providers || currentUser.oauth_providers.length === 0) {
		return emptyLabel;
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

function getDesktopInvoke(): DesktopInvoke | null {
	if (typeof window === 'undefined') return null;
	const desktopWindow = window as DesktopRuntimeWindow;
	const invoke = desktopWindow.__TAURI__?.core?.invoke;
	return typeof invoke === 'function' ? invoke : null;
}

const shellModel = createShellModel(shellState, {
	getApiCapabilities,
	verifyAuth,
	getSettings,
	authLogout,
	isAuthenticated,
	isDesktopRuntime: () => {
		if (typeof window === 'undefined') return false;
		const desktopWindow = window as DesktopWindow;
		return '__TAURI_INTERNALS__' in desktopWindow || desktopWindow.location.protocol === 'tauri:';
	},
	takeLaunchRoute: async () => {
		const invoke = getDesktopInvoke();
		if (!invoke) return null;
		const maybeRoute = await invoke<unknown>('desktop_take_launch_route');
		return typeof maybeRoute === 'string' ? maybeRoute : null;
	},
	getCurrentLocationPath: () => {
		if (typeof window === 'undefined') return currentPath;
		return `${window.location.pathname}${window.location.search}${window.location.hash}`;
	},
	startInterval: (callback, ms) => window.setInterval(callback, ms),
	clearInterval: (handle) => {
		window.clearInterval(handle as number);
	},
	navigate: (path) => onNavigate(path),
});

const navLinks = $derived.by(() => {
	const links: Array<{ href: string; label: string }> = [
		{ href: '/sessions', label: translate($appLocale, 'nav.sessions') },
	];
	links.push({ href: '/docs', label: translate($appLocale, 'nav.docs') });
	if (shellState.desktopRuntime || shellState.hasLocalAuth) {
		links.push({ href: '/settings', label: translate($appLocale, 'nav.settings') });
	}
	return links;
});

$effect(() => {
	void shellModel.loadCapabilities();
});

$effect(() => {
	void shellState.desktopRuntime;
	if (!shellState.desktopRuntime) return;
	return shellModel.startLaunchRoutePolling();
});

const shortcutHints = $derived.by(() => {
	if (isSessionDetail) {
		return [
			`Cmd/Ctrl+K ${translate($appLocale, 'shortcut.palette')}`,
			`j/k ${translate($appLocale, 'shortcut.scroll')}`,
			`1-0 ${translate($appLocale, 'shortcut.filters')}`,
			`/ ${translate($appLocale, 'shortcut.search')}`,
			`n/p ${translate($appLocale, 'shortcut.match')}`,
			`Esc ${translate($appLocale, 'shortcut.back')}`,
		];
	}
	if (isSessionList) {
		return [
			`Cmd/Ctrl+K ${translate($appLocale, 'shortcut.palette')}`,
			`j/k ${translate($appLocale, 'shortcut.navigate')}`,
			`Enter ${translate($appLocale, 'shortcut.open')}`,
			`/ ${translate($appLocale, 'shortcut.search')}`,
			`t ${translate($appLocale, 'shortcut.tool')}`,
			`r ${translate($appLocale, 'shortcut.range')}`,
			`g ${translate($appLocale, 'shortcut.repo')}`,
		];
	}
	return [
		`Cmd/Ctrl+K ${translate($appLocale, 'shortcut.palette')}`,
		`Esc ${translate($appLocale, 'shortcut.back')}`,
	];
});

$effect(() => {
	void currentPath;
	void shellState.authEnabled;
	void shellModel.loadUser();
});

$effect(() => {
	void currentPath;
	shellModel.resetMenusForPath();
});

onMount(() => {
	initializeLocalization();
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
			translate($appLocale, 'palette.goSessions.label'),
			translate($appLocale, 'palette.goSessions.description'),
			['sessions', 'home', 'list', '/sessions'],
			() => onNavigate('/sessions'),
		),
		createPaletteCommand(
			'go-docs',
			translate($appLocale, 'palette.goDocs.label'),
			translate($appLocale, 'palette.goDocs.description'),
			['docs', 'documentation', 'guide'],
			() => onNavigate('/docs'),
		),
	];

	if (!shellState.hasLocalAuth && (!shellState.desktopRuntime || shellState.authEnabled)) {
		commands.push(
			createPaletteCommand(
				'go-login',
				translate($appLocale, 'palette.goLogin.label'),
				translate($appLocale, 'palette.goLogin.description'),
				['login', 'auth', 'signin'],
				() => onNavigate('/login'),
			),
		);
	}
	if (shellState.hasLocalAuth || shellState.desktopRuntime) {
		commands.push(
			createPaletteCommand(
				'go-settings',
				translate($appLocale, 'palette.goSettings.label'),
				translate($appLocale, 'palette.goSettings.description'),
				['settings', 'account', 'profile', 'api key'],
				() => onNavigate('/settings'),
			),
		);
	}

	if (isSessionList) {
		commands.push(
			createPaletteCommand(
				'focus-list-search',
				translate($appLocale, 'palette.focusSessionSearch.label'),
				translate($appLocale, 'palette.focusSessionSearch.description'),
				['search', 'find', 'session list'],
				dispatchFocusSearch,
			),
		);
	}

	if (isSessionDetail) {
		commands.push(
			createPaletteCommand(
				'focus-detail-search',
				translate($appLocale, 'palette.focusDetailSearch.label'),
				translate($appLocale, 'palette.focusDetailSearch.description'),
				['search', 'find', 'timeline', 'session detail'],
				dispatchFocusSearch,
			),
		);
	}

	return commands;
});

const normalizedPaletteQuery = $derived(shellState.paletteQuery.trim().toLowerCase());
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
	shellModel.resetPaletteSelection();
});

$effect(() => {
	shellModel.clampPaletteSelection(visiblePaletteCommands.length - 1);
});

function isLinkActive(href: string): boolean {
	if (href === '/sessions')
		return currentPath === '/sessions' || currentPath.startsWith('/session/');
	return currentPath === href;
}

function isPaletteShortcut(e: KeyboardEvent): boolean {
	return e.key.toLowerCase() === 'k' && (e.metaKey || e.ctrlKey);
}

function isHelpShortcut(e: KeyboardEvent): boolean {
	return !e.metaKey && !e.ctrlKey && !e.altKey && e.key.toLowerCase() === 'h';
}

function isEditableTarget(target: EventTarget | null): boolean {
	if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) return true;
	return target instanceof HTMLElement && target.isContentEditable;
}

async function openPalette() {
	shellModel.openPalette();
	await tick();
	paletteInput?.focus();
}

function closePalette() {
	shellModel.closePalette();
}

function openHelp() {
	shellModel.openHelp();
}

function closeHelp() {
	shellModel.closeHelp();
}

function trapHelpFocus(e: KeyboardEvent) {
	if (!helpDialog || e.key !== 'Tab') return;
	const focusables = Array.from(
		helpDialog.querySelectorAll<HTMLElement>(
			'button,[href],input,select,textarea,[tabindex]:not([tabindex="-1"])',
		),
	).filter((element) => !element.hasAttribute('disabled'));
	if (focusables.length === 0) return;
	const first = focusables[0];
	const last = focusables[focusables.length - 1];
	const active = document.activeElement;
	if (e.shiftKey) {
		if (active === first || !helpDialog.contains(active)) {
			e.preventDefault();
			last.focus();
		}
		return;
	}
	if (active === last) {
		e.preventDefault();
		first.focus();
	}
}

function closeAccountMenu() {
	shellModel.closeAccountMenu();
}

function toggleAccountMenu() {
	shellModel.toggleAccountMenu();
}

function handleWindowPointerDown(e: MouseEvent) {
	if (!shellState.accountMenuOpen) return;
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
	shellModel.movePaletteSelection(direction, visiblePaletteCommands.length);
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
		executePaletteCommand(visiblePaletteCommands[shellState.paletteSelectionIndex]);
		return;
	}
	if (e.key === 'Escape') {
		e.preventDefault();
		closePalette();
	}
}

async function handleSignOut() {
	await shellModel.signOut();
}

function handleAccountMenuNavigate(path: string) {
	shellModel.closeAccountMenu();
	onNavigate(path);
}

function handleGlobalKey(e: KeyboardEvent) {
	if (isHelpShortcut(e)) {
		if (isEditableTarget(e.target)) return;
		e.preventDefault();
		if (shellState.helpOpen) closeHelp();
		else openHelp();
		return;
	}

	if (isPaletteShortcut(e)) {
		e.preventDefault();
		if (shellState.paletteOpen) {
			closePalette();
		} else {
			void openPalette();
		}
		return;
	}

	if (shellState.helpOpen) {
		if (e.key === 'Escape') {
			e.preventDefault();
			closeHelp();
			return;
		}
		trapHelpFocus(e);
		return;
	}

	if (shellState.accountMenuOpen && e.key === 'Escape') {
		e.preventDefault();
		closeAccountMenu();
		return;
	}

	if (shellState.paletteOpen) {
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

function handleLanguageChange() {
	refreshLocaleFromPlatform();
}
</script>

<svelte:window
	onkeydown={handleGlobalKey}
	onmousedown={handleWindowPointerDown}
	onlanguagechange={handleLanguageChange}
/>

<div class="grid h-[100dvh] max-w-[100vw] grid-rows-[auto_1fr_auto] overflow-hidden bg-bg-primary text-text-primary">
	<nav class="relative z-30 flex min-w-0 flex-wrap items-center justify-between gap-2 border-b border-border bg-bg-secondary px-2 py-2 sm:px-4">
		<div class="flex items-center gap-1">
			<a href="/" class="text-sm font-bold tracking-tight text-text-primary sm:text-base">
				opensession<span class="text-accent">.io</span>
			</a>
			<LanguageModePicker compact />
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

			{#if shellState.hasLocalAuth}
				<div class="relative shrink-0" bind:this={accountMenuRoot}>
					<button
						type="button"
						data-testid="account-menu-trigger"
						aria-expanded={shellState.accountMenuOpen}
						aria-haspopup="menu"
						onclick={toggleAccountMenu}
						class="px-1.5 py-1 text-xs text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary sm:px-3 sm:text-sm"
					>
						[{navAccountHandle(shellState.user)}]
					</button>

						{#if shellState.accountMenuOpen}
							<div
								role="menu"
								aria-label={translate($appLocale, 'account.menu')}
								data-testid="account-menu"
								class="absolute right-0 z-40 mt-1 w-[min(18rem,calc(100vw-1rem))] border border-border bg-bg-primary shadow-2xl"
							>
							<div class="border-b border-border px-3 py-2">
								<p class="text-[11px] uppercase tracking-[0.1em] text-text-muted">
									{translate($appLocale, 'account.title')}
								</p>
								<p class="mt-1 text-sm font-medium text-text-primary">{shellState.user?.nickname}</p>
								<p class="text-xs text-text-secondary">
									{shellState.user?.email ?? translate($appLocale, 'account.emailMissing')}
								</p>
							</div>

							<div class="border-b border-border px-3 py-2 text-xs text-text-secondary">
								<p>
									{translate($appLocale, 'account.userId')}:
									<span class="text-text-primary">{shellState.user?.user_id}</span>
								</p>
								<p>
									{translate($appLocale, 'account.joined')}:
									<span class="text-text-primary">{shortDate(shellState.user?.created_at)}</span>
								</p>
								<p>
									{translate($appLocale, 'account.providers')}:
									<span class="text-text-primary">
										{linkedProvidersLabel(shellState.user, translate($appLocale, 'common.none'))}
									</span>
								</p>
							</div>

								<div class="border-b border-border px-2 py-1">
									<button
										type="button"
										class="block w-full px-2 py-1 text-left text-xs text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary"
										onclick={() => handleAccountMenuNavigate('/settings')}
									>
										{translate($appLocale, 'nav.settings')}
									</button>
									<button
										type="button"
									class="block w-full px-2 py-1 text-left text-xs text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary"
								onclick={() => handleAccountMenuNavigate('/sessions')}
							>
									{translate($appLocale, 'account.sessionHome')}
								</button>
								<button
									type="button"
									class="block w-full px-2 py-1 text-left text-xs text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary"
									onclick={() => handleAccountMenuNavigate('/docs')}
								>
									{translate($appLocale, 'nav.docs')}
								</button>
							</div>

							<div class="px-2 py-1">
								<button
									type="button"
									data-testid="account-menu-logout"
									onclick={handleSignOut}
									class="block w-full px-2 py-1 text-left text-xs text-error transition-colors hover:bg-error/10"
								>
									{translate($appLocale, 'account.logout')}
								</button>
							</div>
						</div>
					{/if}
				</div>
			{:else if showLoginLink}
				<a
					href="/login"
					class="px-1.5 py-1 text-xs text-text-secondary transition-colors hover:bg-bg-hover hover:text-text-primary sm:px-3 sm:text-sm"
				>
					{translate($appLocale, 'nav.login')}
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
			{translate($appLocale, 'footer.shortcuts')}
		</span>
		<span class="sm:hidden inline-flex items-center gap-1 text-text-secondary">
			<kbd class="font-mono text-[10px] font-semibold text-accent">
				Cmd/Ctrl+K
			</kbd>
		</span>
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
		<span class="hidden sm:inline-flex items-center gap-1 text-text-secondary">
			<kbd class="font-mono text-[10px] font-semibold text-accent">h</kbd>
			<span>{translate($appLocale, 'footer.help')}</span>
		</span>
		<span class="ml-auto inline-flex items-center gap-2 whitespace-nowrap">
			<a
				href={OPENSESSION_GITHUB_URL}
				target="_blank"
				rel="noreferrer"
				aria-label={translate($appLocale, 'footer.githubAria')}
				data-testid="footer-github-link"
				class="text-text-secondary transition-colors hover:text-text-primary"
			>
				{translate($appLocale, 'footer.github')}
			</a>
			<span aria-hidden="true" class="text-border">/</span>
			<span>opensession.io</span>
		</span>
	</footer>
</div>

{#if shellState.paletteOpen}
	<div class="fixed inset-0 z-50 p-4">
		<button
			type="button"
			class="absolute inset-0 bg-black/60"
			aria-label={translate($appLocale, 'help.commandPaletteClose')}
			onclick={closePalette}
		></button>
		<div
			role="dialog"
			aria-modal="true"
			aria-label={translate($appLocale, 'help.commandPalette')}
			tabindex="-1"
			data-testid="command-palette"
			class="relative mx-auto mt-14 w-full max-w-2xl border border-border bg-bg-primary shadow-2xl"
		>
			<div class="border-b border-border px-3 py-2">
				<input
					bind:this={paletteInput}
					bind:value={shellState.paletteQuery}
					onkeydown={handlePaletteInputKeydown}
					data-testid="command-palette-input"
					type="text"
					placeholder={translate($appLocale, 'palette.placeholder')}
					class="w-full border border-border bg-bg-secondary px-2 py-1 text-sm text-text-primary outline-none focus:border-accent"
				/>
			</div>
			<div class="max-h-[24rem] overflow-y-auto">
				{#if visiblePaletteCommands.length === 0}
					<p class="px-3 py-3 text-xs text-text-muted">
						{translate($appLocale, 'palette.noMatches')}
					</p>
				{:else}
					{#each visiblePaletteCommands as command, idx (command.id)}
						<button
							type="button"
							class="flex w-full items-start justify-between gap-3 border-b border-border/60 px-3 py-2 text-left"
							class:bg-bg-secondary={idx === shellState.paletteSelectionIndex}
							class:hover:bg-bg-secondary={idx !== shellState.paletteSelectionIndex}
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
				<span>
					{translate($appLocale, 'palette.commandCount', { count: visiblePaletteCommands.length })}
				</span>
				<span>{translate($appLocale, 'palette.footer')}</span>
			</div>
		</div>
	</div>
{/if}

{#if shellState.helpOpen}
	<div class="fixed inset-0 z-50 p-4">
		<button
			type="button"
			class="absolute inset-0 bg-black/65"
			aria-label={translate($appLocale, 'help.overlayClose')}
			onclick={closeHelp}
		></button>
		<div
			role="dialog"
			aria-modal="true"
			aria-label={translate($appLocale, 'help.quickHelp')}
			tabindex="-1"
			data-testid="keyboard-help-modal"
			bind:this={helpDialog}
			class="relative mx-auto mt-10 w-full max-w-3xl border border-border bg-bg-primary shadow-2xl"
		>
			<div class="flex items-center justify-between border-b border-border px-4 py-3">
				<div>
					<p class="text-[11px] uppercase tracking-[0.1em] text-text-muted">
						{translate($appLocale, 'help.quickHelp')}
					</p>
					<h2 class="text-sm font-semibold text-text-primary">
						{translate($appLocale, 'help.sessionRuntimeGuide')}
					</h2>
				</div>
				<button
					type="button"
					onclick={closeHelp}
					class="border border-border px-2 py-1 text-xs text-text-secondary hover:text-text-primary"
				>
					{translate($appLocale, 'common.close')}
				</button>
			</div>
			<div class="grid gap-3 p-4 sm:grid-cols-3">
				<section class="rounded border border-border/70 bg-bg-secondary/60 p-3">
					<h3 class="text-xs font-semibold uppercase tracking-[0.08em] text-text-muted">
						{translate($appLocale, 'help.runtimeSummary')}
					</h3>
					<ul class="mt-2 space-y-1 text-xs text-text-secondary">
						<li>{translate($appLocale, 'help.runtimeProvider')}</li>
						<li>{translate($appLocale, 'help.runtimeShape')}</li>
						<li>{translate($appLocale, 'help.runtimePrompt')}</li>
					</ul>
				</section>
				<section class="rounded border border-border/70 bg-bg-secondary/60 p-3">
					<h3 class="text-xs font-semibold uppercase tracking-[0.08em] text-text-muted">
						{translate($appLocale, 'help.vector')}
					</h3>
					<ul class="mt-2 space-y-1 text-xs text-text-secondary">
						<li>{translate($appLocale, 'help.vectorAutoChunk')}</li>
						<li>{translate($appLocale, 'help.vectorManual')}</li>
						<li>{translate($appLocale, 'help.vectorFix')}</li>
					</ul>
				</section>
				<section class="rounded border border-border/70 bg-bg-secondary/60 p-3">
					<h3 class="text-xs font-semibold uppercase tracking-[0.08em] text-text-muted">
						{translate($appLocale, 'help.changeReader')}
					</h3>
					<ul class="mt-2 space-y-1 text-xs text-text-secondary">
						<li>{translate($appLocale, 'help.changeReaderText')}</li>
						<li>{translate($appLocale, 'help.changeReaderVoice')}</li>
						<li>{translate($appLocale, 'help.changeReaderApiKey')}</li>
					</ul>
				</section>
			</div>
			<div class="flex items-center justify-between border-t border-border px-4 py-2 text-[11px] text-text-muted">
				<span>{translate($appLocale, 'help.footerShortcuts')}</span>
				<span>{translate($appLocale, 'help.footerFocus')}</span>
			</div>
		</div>
	</div>
{/if}
