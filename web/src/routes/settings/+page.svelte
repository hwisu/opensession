<script lang="ts">
	import { getSettings, register, regenerateApiKey, setApiKey, clearApiKey } from '$lib/api';
	import type { UserSettings } from '$lib/types';

	let settings = $state<UserSettings | null>(null);
	let loading = $state(true);
	let error = $state<string | null>(null);

	// Register form
	let nickname = $state('');
	let registering = $state(false);
	let registerError = $state<string | null>(null);

	// API key input
	let manualKey = $state('');
	let showKey = $state(false);
	let regenerating = $state(false);

	async function fetchSettings() {
		loading = true;
		error = null;
		try {
			settings = await getSettings();
		} catch {
			settings = null;
		} finally {
			loading = false;
		}
	}

	async function handleRegister() {
		if (!nickname.trim()) return;
		registering = true;
		registerError = null;
		try {
			const result = await register(nickname.trim());
			setApiKey(result.api_key);
			await fetchSettings();
		} catch (e) {
			registerError = e instanceof Error ? e.message : 'Registration failed';
		} finally {
			registering = false;
		}
	}

	function handleSetKey() {
		if (manualKey.trim()) {
			setApiKey(manualKey.trim());
			manualKey = '';
			fetchSettings();
		}
	}

	function handleLogout() {
		clearApiKey();
		settings = null;
	}

	async function handleRegenerateKey() {
		regenerating = true;
		try {
			const result = await regenerateApiKey();
			setApiKey(result.api_key);
			if (settings) {
				settings = { ...settings, api_key: result.api_key };
			}
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to regenerate key';
		} finally {
			regenerating = false;
		}
	}

	function getGithubLoginUrl(): string {
		return `${window.location.origin}/api/auth/github`;
	}

	$effect(() => {
		fetchSettings();
	});
</script>

<svelte:head>
	<title>Settings - opensession.io</title>
</svelte:head>

<div class="mx-auto max-w-2xl">
	<h1 class="mb-6 text-2xl font-bold text-white">Settings</h1>

	{#if loading}
		<div class="py-16 text-center text-sm text-text-muted">Loading...</div>
	{:else if settings}
		<!-- Authenticated settings -->
		<div class="space-y-6">
			<!-- Profile -->
			<div class="rounded-lg border border-border bg-bg-secondary p-4">
				<h2 class="mb-3 text-sm font-medium text-text-primary">Profile</h2>
				<div class="flex items-center justify-between">
					<div class="flex items-center gap-3">
						{#if settings.avatar_url}
							<img
								src={settings.avatar_url}
								alt={settings.nickname}
								class="h-10 w-10 rounded-full"
							/>
						{:else}
							<div class="flex h-10 w-10 items-center justify-center rounded-full bg-bg-hover text-sm font-bold text-text-secondary">
								{settings.nickname[0].toUpperCase()}
							</div>
						{/if}
						<div>
							<p class="text-sm text-text-primary">{settings.nickname}</p>
							{#if settings.github_login}
								<p class="text-xs text-text-muted">@{settings.github_login} on GitHub</p>
							{/if}
							<p class="text-xs text-text-muted">User ID: {settings.user_id}</p>
						</div>
					</div>
					<button
						onclick={handleLogout}
						class="rounded bg-bg-hover px-3 py-1 text-xs text-text-secondary hover:text-text-primary"
					>
						Logout
					</button>
				</div>
				{#if !settings.github_login}
					<div class="mt-3 border-t border-border pt-3">
						<a
							href={getGithubLoginUrl()}
							class="inline-flex items-center gap-2 rounded-lg bg-[#24292f] px-4 py-2 text-sm font-medium text-white hover:bg-[#32383f]"
						>
							<svg class="h-5 w-5" fill="currentColor" viewBox="0 0 24 24"><path d="M12 0C5.37 0 0 5.37 0 12c0 5.31 3.435 9.795 8.205 11.385.6.105.825-.255.825-.57 0-.285-.015-1.23-.015-2.235-3.015.555-3.795-.735-4.035-1.41-.135-.345-.72-1.41-1.23-1.695-.42-.225-1.02-.78-.015-.795.945-.015 1.62.87 1.845 1.23 1.08 1.815 2.805 1.305 3.495.99.105-.78.42-1.305.765-1.605-2.67-.3-5.46-1.335-5.46-5.925 0-1.305.465-2.385 1.23-3.225-.12-.3-.54-1.53.12-3.18 0 0 1.005-.315 3.3 1.23.96-.27 1.98-.405 3-.405s2.04.135 3 .405c2.295-1.56 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.765.84 1.23 1.905 1.23 3.225 0 4.605-2.805 5.625-5.475 5.925.435.375.81 1.095.81 2.22 0 1.605-.015 2.895-.015 3.3 0 .315.225.69.825.57A12.02 12.02 0 0024 12c0-6.63-5.37-12-12-12z"/></svg>
							Link GitHub Account
						</a>
					</div>
				{/if}
			</div>

			<!-- API Key -->
			<div class="rounded-lg border border-border bg-bg-secondary p-4">
				<h2 class="mb-3 text-sm font-medium text-text-primary">API Key</h2>
				<div class="mb-3">
					<div class="flex items-center gap-2">
						<code class="flex-1 rounded bg-bg-primary px-3 py-2 text-xs text-text-secondary">
							{showKey ? settings.api_key : '••••••••••••••••'}
						</code>
						<button
							onclick={() => (showKey = !showKey)}
							class="rounded bg-bg-hover px-3 py-2 text-xs text-text-secondary hover:text-text-primary"
						>
							{showKey ? 'Hide' : 'Show'}
						</button>
						<button
							onclick={() => navigator.clipboard.writeText(settings!.api_key)}
							class="rounded bg-bg-hover px-3 py-2 text-xs text-text-secondary hover:text-text-primary"
						>
							Copy
						</button>
					</div>
				</div>
				<button
					onclick={handleRegenerateKey}
					disabled={regenerating}
					class="rounded bg-error/10 px-3 py-1.5 text-xs text-error hover:bg-error/20 disabled:opacity-50"
				>
					{regenerating ? 'Regenerating...' : 'Regenerate Key'}
				</button>
			</div>

			<!-- Daemon setup -->
			<div class="rounded-lg border border-border bg-bg-secondary p-4">
				<h2 class="mb-3 text-sm font-medium text-text-primary">Daemon Setup</h2>
				<p class="mb-3 text-xs text-text-secondary">
					The opensession daemon watches your AI tool sessions and automatically uploads them.
				</p>
				<div class="space-y-2 rounded bg-bg-primary p-3 font-mono text-xs text-text-secondary">
					<p class="text-text-muted"># Install</p>
					<p>cargo install opensession-cli</p>
					<p class="mt-2 text-text-muted"># Configure</p>
					<p>opensession config set api-key {showKey ? settings.api_key : '<your-api-key>'}</p>
					<p class="mt-2 text-text-muted"># Start daemon</p>
					<p>opensession daemon start</p>
				</div>
			</div>
		</div>
	{:else}
		<!-- Not authenticated -->
		<div class="space-y-6">
			<!-- GitHub sign in -->
			<div class="rounded-lg border border-border bg-bg-secondary p-4">
				<h2 class="mb-3 text-sm font-medium text-text-primary">Sign in with GitHub</h2>
				<p class="mb-3 text-xs text-text-secondary">
					Connect your GitHub account to upload sessions and join groups.
				</p>
				<a
					href={getGithubLoginUrl()}
					class="inline-flex items-center gap-2 rounded-lg bg-[#24292f] px-4 py-2 text-sm font-medium text-white hover:bg-[#32383f]"
				>
					<svg class="h-5 w-5" fill="currentColor" viewBox="0 0 24 24"><path d="M12 0C5.37 0 0 5.37 0 12c0 5.31 3.435 9.795 8.205 11.385.6.105.825-.255.825-.57 0-.285-.015-1.23-.015-2.235-3.015.555-3.795-.735-4.035-1.41-.135-.345-.72-1.41-1.23-1.695-.42-.225-1.02-.78-.015-.795.945-.015 1.62.87 1.845 1.23 1.08 1.815 2.805 1.305 3.495.99.105-.78.42-1.305.765-1.605-2.67-.3-5.46-1.335-5.46-5.925 0-1.305.465-2.385 1.23-3.225-.12-.3-.54-1.53.12-3.18 0 0 1.005-.315 3.3 1.23.96-.27 1.98-.405 3-.405s2.04.135 3 .405c2.295-1.56 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.765.84 1.23 1.905 1.23 3.225 0 4.605-2.805 5.625-5.475 5.925.435.375.81 1.095.81 2.22 0 1.605-.015 2.895-.015 3.3 0 .315.225.69.825.57A12.02 12.02 0 0024 12c0-6.63-5.37-12-12-12z"/></svg>
					Sign in with GitHub
				</a>
			</div>

			<!-- Register with nickname -->
			<div class="rounded-lg border border-border bg-bg-secondary p-4">
				<h2 class="mb-3 text-sm font-medium text-text-primary">Or register with nickname</h2>
				<p class="mb-3 text-xs text-text-secondary">
					Create an account without GitHub.
				</p>
				<div class="flex gap-2">
					<input
						type="text"
						placeholder="Choose a nickname"
						bind:value={nickname}
						onkeydown={(e) => e.key === 'Enter' && handleRegister()}
						class="flex-1 rounded-lg border border-border bg-bg-primary px-3 py-2 text-sm text-text-primary placeholder-text-muted outline-none focus:border-accent"
					/>
					<button
						onclick={handleRegister}
						disabled={registering || !nickname.trim()}
						class="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white hover:bg-accent/80 disabled:opacity-50"
					>
						{registering ? '...' : 'Register'}
					</button>
				</div>
				{#if registerError}
					<p class="mt-2 text-sm text-error">{registerError}</p>
				{/if}
			</div>

			<div class="rounded-lg border border-border bg-bg-secondary p-4">
				<h2 class="mb-3 text-sm font-medium text-text-primary">Already have an API key?</h2>
				<div class="flex gap-2">
					<input
						type="text"
						placeholder="Paste your API key"
						bind:value={manualKey}
						onkeydown={(e) => e.key === 'Enter' && handleSetKey()}
						class="flex-1 rounded-lg border border-border bg-bg-primary px-3 py-2 text-sm text-text-primary placeholder-text-muted outline-none focus:border-accent"
					/>
					<button
						onclick={handleSetKey}
						disabled={!manualKey.trim()}
						class="rounded-lg bg-bg-hover px-4 py-2 text-sm text-text-secondary hover:text-text-primary disabled:opacity-50"
					>
						Set Key
					</button>
				</div>
			</div>
		</div>
	{/if}

	{#if error}
		<div class="mt-4 rounded-lg border border-error/30 bg-error/10 px-4 py-3 text-sm text-error">
			{error}
		</div>
	{/if}
</div>
