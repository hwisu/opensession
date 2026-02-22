<script lang="ts">
import { ApiError, getSettings, isAuthenticated, issueApiKey } from '../api';
import type { UserSettings } from '../types';

const {
	onNavigate = (path: string) => {
		window.location.assign(path);
	},
}: {
	onNavigate?: (path: string) => void;
} = $props();

let settings = $state<UserSettings | null>(null);
let loading = $state(true);
let error = $state<string | null>(null);
let issuing = $state(false);
let issuedApiKey = $state<string | null>(null);
let copyMessage = $state<string | null>(null);
let authRequired = $state(false);

function formatDate(value: string | null | undefined): string {
	if (!value) return '-';
	const parsed = new Date(value);
	if (Number.isNaN(parsed.getTime())) return '-';
	return parsed.toLocaleString();
}

function normalizeError(err: unknown, fallback: string): string {
	if (err instanceof ApiError) return err.message || fallback;
	if (err instanceof Error) return err.message || fallback;
	return fallback;
}

async function loadSettings() {
	if (!isAuthenticated()) {
		authRequired = true;
		loading = false;
		return;
	}

	loading = true;
	error = null;
	authRequired = false;
	try {
		settings = await getSettings();
	} catch (err) {
		settings = null;
		if (err instanceof ApiError && (err.status === 401 || err.status === 403)) {
			authRequired = true;
		} else {
			error = normalizeError(err, 'Failed to load settings');
		}
	} finally {
		loading = false;
	}
}

async function handleIssueApiKey() {
	issuing = true;
	error = null;
	copyMessage = null;
	try {
		const response = await issueApiKey();
		issuedApiKey = response.api_key;
	} catch (err) {
		error = normalizeError(err, 'Failed to issue API key');
	} finally {
		issuing = false;
	}
}

async function copyApiKey() {
	if (!issuedApiKey) return;
	try {
		await navigator.clipboard.writeText(issuedApiKey);
		copyMessage = 'Copied';
	} catch {
		copyMessage = 'Copy failed';
	}
}

$effect(() => {
	loadSettings();
});
</script>

<svelte:head>
	<title>Settings - opensession.io</title>
</svelte:head>

<div data-testid="settings-page" class="mx-auto w-full max-w-3xl space-y-4 pb-10">
	<header class="border border-border bg-bg-secondary px-4 py-3">
		<p class="text-[11px] uppercase tracking-[0.12em] text-text-muted">Account</p>
		<h1 class="mt-1 text-3xl font-semibold tracking-tight text-text-primary">Settings</h1>
		<p class="mt-1 text-sm text-text-secondary">Personal profile and API access controls.</p>
	</header>

	{#if loading}
		<div class="border border-border bg-bg-secondary px-4 py-8 text-center text-sm text-text-muted">Loading...</div>
	{:else if authRequired}
		<section
			data-testid="settings-require-auth"
			class="border border-border bg-bg-secondary px-4 py-6 text-sm text-text-secondary"
		>
			<p class="text-text-primary">Sign in is required to view personal settings.</p>
			<div class="mt-4">
				<button
					type="button"
					onclick={() => onNavigate('/login')}
					class="bg-accent px-3 py-2 text-xs font-semibold text-white hover:bg-accent/85"
				>
					Go to login
				</button>
			</div>
		</section>
	{:else}
		<section class="border border-border bg-bg-secondary p-4">
			<h2 class="text-sm font-semibold text-text-primary">Profile</h2>
			{#if settings}
				<dl class="mt-3 grid gap-2 text-xs text-text-secondary sm:grid-cols-[10rem_1fr]">
					<dt>User ID</dt>
					<dd class="font-mono text-text-primary">{settings.user_id}</dd>
					<dt>Nickname</dt>
					<dd class="text-text-primary">{settings.nickname}</dd>
					<dt>Email</dt>
					<dd class="text-text-primary">{settings.email ?? 'not linked'}</dd>
					<dt>Joined</dt>
					<dd class="text-text-primary">{formatDate(settings.created_at)}</dd>
					<dt>Linked OAuth</dt>
					<dd class="text-text-primary">
						{#if settings.oauth_providers.length === 0}
							none
						{:else}
							{settings.oauth_providers.map((provider) => provider.display_name).join(', ')}
						{/if}
					</dd>
				</dl>
			{:else}
				<p class="mt-2 text-xs text-text-muted">No profile data available.</p>
			{/if}
		</section>

		<section class="border border-border bg-bg-secondary p-4">
			<div class="flex flex-wrap items-center justify-between gap-3">
				<div>
					<h2 class="text-sm font-semibold text-text-primary">Personal API Key</h2>
					<p class="mt-1 text-xs text-text-secondary">
						Issue a new key for CLI and automation access. Existing active key moves to grace mode.
					</p>
				</div>
				<button
					type="button"
					data-testid="issue-api-key-button"
					onclick={handleIssueApiKey}
					disabled={issuing}
					class="bg-accent px-3 py-2 text-xs font-semibold text-white hover:bg-accent/85 disabled:opacity-60"
				>
					{issuing ? 'Issuing...' : issuedApiKey ? 'Regenerate key' : 'Issue key'}
				</button>
			</div>

			{#if issuedApiKey}
				<div class="mt-4 border border-border/80 bg-bg-primary p-3">
					<p class="mb-2 text-xs text-text-muted">Shown once. Save this key now.</p>
					<code data-testid="issued-api-key" class="block break-all font-mono text-xs text-text-primary">
						{issuedApiKey}
					</code>
					<div class="mt-3 flex items-center gap-2">
						<button
							type="button"
							data-testid="copy-api-key"
							onclick={copyApiKey}
							class="border border-border px-2 py-1 text-xs text-text-secondary hover:text-text-primary"
						>
							Copy
						</button>
						{#if copyMessage}
							<span class="text-xs text-text-muted">{copyMessage}</span>
						{/if}
					</div>
				</div>
			{/if}
		</section>

		{#if error}
			<p class="text-xs text-error">{error}</p>
		{/if}
	{/if}
</div>
