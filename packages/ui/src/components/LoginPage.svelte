<script lang="ts">
import { ApiError, authLogin, authRegister, getAuthProviders, getOAuthUrl } from '../api';
import type { OAuthProviderInfo } from '../types';

const {
	onSuccess = () => {},
}: {
	onSuccess?: () => void;
} = $props();

let email = $state('');
let password = $state('');
let loading = $state(false);
let error = $state<string | null>(null);
let emailPasswordEnabled = $state(true);
let oauthProviders = $state<OAuthProviderInfo[]>([]);
let authUnavailable = $state(false);
let nickname = $state('');

function normalizeNickname(rawEmail: string, rawNickname: string): string {
	const explicit = rawNickname.trim();
	if (explicit) return explicit.slice(0, 64);
	const localPart = rawEmail.trim().toLowerCase().split('@')[0] ?? '';
	const compact = localPart.replace(/[^a-z0-9_-]/g, '');
	if (compact) return compact.slice(0, 64);
	return `user-${crypto.randomUUID().slice(0, 8)}`;
}

$effect(() => {
	getAuthProviders()
		.then((resp) => {
			emailPasswordEnabled = resp.email_password;
			oauthProviders = resp.oauth;
			authUnavailable = !resp.email_password && resp.oauth.length === 0;
		})
		.catch(() => {
			emailPasswordEnabled = false;
			oauthProviders = [];
			authUnavailable = true;
		});
});

async function handleSubmit() {
	if (!email.trim() || !password) return;
	loading = true;
	error = null;
	try {
		await authLogin(email.trim(), password);
		onSuccess();
	} catch (e) {
		if (e instanceof ApiError && e.status === 401) {
			try {
				await authRegister(email.trim(), password, normalizeNickname(email, nickname));
				onSuccess();
				return;
			} catch (registerError) {
				if (registerError instanceof ApiError && registerError.status === 409) {
					error = 'Invalid email or password';
				} else {
					error = registerError instanceof Error ? registerError.message : 'Authentication failed';
				}
				return;
			}
		}
		error = e instanceof Error ? e.message : 'Authentication failed';
	} finally {
		loading = false;
	}
}
</script>

<svelte:head>
	<title>Sign In - opensession.io</title>
</svelte:head>

<div class="mx-auto w-full max-w-sm px-3 py-10 sm:px-0">
	<h1 class="mb-6 text-center text-lg font-bold text-text-primary">Sign In</h1>
	<p class="mb-4 text-center text-xs text-text-muted">New accounts are created automatically on first sign in.</p>

	{#if emailPasswordEnabled}
		<form onsubmit={(e) => { e.preventDefault(); handleSubmit(); }} class="space-y-3">
			<div>
				<label for="login-email" class="sr-only">Email</label>
				<input
					id="login-email"
					type="email"
					placeholder="Email"
					bind:value={email}
					class="w-full border border-border bg-bg-primary px-3 py-2 text-xs text-text-primary placeholder-text-muted outline-none focus:border-accent"
				/>
			</div>
			<div>
				<label for="login-password" class="sr-only">Password</label>
				<input
					id="login-password"
					type="password"
					placeholder="Password"
					bind:value={password}
					class="w-full border border-border bg-bg-primary px-3 py-2 text-xs text-text-primary placeholder-text-muted outline-none focus:border-accent"
				/>
			</div>
			<div>
				<label for="login-nickname" class="sr-only">Nickname (optional)</label>
				<input
					id="login-nickname"
					type="text"
					placeholder="Nickname (optional, first sign in only)"
					bind:value={nickname}
					class="w-full border border-border bg-bg-primary px-3 py-2 text-xs text-text-primary placeholder-text-muted outline-none focus:border-accent"
				/>
			</div>

			{#if error}
				<p class="text-xs text-error">{error}</p>
			{/if}

			<button
				type="submit"
				disabled={loading || !email.trim() || !password}
				class="w-full bg-accent px-3 py-2 text-xs font-medium text-white hover:bg-accent/80 disabled:opacity-50"
			>
				{loading ? 'Signing in...' : 'Continue'}
			</button>
		</form>
	{/if}

	{#if emailPasswordEnabled && oauthProviders.length > 0}
		<div class="my-4 flex items-center gap-3">
			<div class="flex-1 border-t border-border"></div>
			<span class="text-xs text-text-muted">or</span>
			<div class="flex-1 border-t border-border"></div>
		</div>
	{/if}

	{#each oauthProviders as provider}
		<a
			href={getOAuthUrl(provider.id)}
			class="mb-2 block w-full border border-border bg-bg-secondary px-3 py-2 text-center text-xs text-text-primary hover:bg-bg-hover"
		>
			Continue with {provider.display_name}
		</a>
	{/each}

	{#if authUnavailable}
		<p data-testid="auth-unavailable" class="mb-3 text-xs text-text-muted">
			Authentication is not available in this deployment.
		</p>
	{/if}
</div>
