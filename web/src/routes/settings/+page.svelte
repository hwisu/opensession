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
						<div class="flex h-10 w-10 items-center justify-center rounded-full bg-bg-hover text-sm font-bold text-text-secondary">
							{settings.nickname[0].toUpperCase()}
						</div>
						<div>
							<p class="text-sm text-text-primary">{settings.nickname}</p>
							{#if settings.is_admin}
								<p class="text-xs text-accent">Admin</p>
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
			<!-- Register with nickname -->
			<div class="rounded-lg border border-border bg-bg-secondary p-4">
				<h2 class="mb-3 text-sm font-medium text-text-primary">Register</h2>
				<p class="mb-3 text-xs text-text-secondary">
					Choose a nickname to create your account. The first user becomes the admin.
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
