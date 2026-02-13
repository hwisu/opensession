<script lang="ts">
import { authRegister, getAuthProviders, getOAuthUrl } from '../api';
import type { OAuthProviderInfo } from '../types';

const {
	onSuccess = () => {},
	onNavigate = (_path: string) => {},
}: {
	onSuccess?: () => void;
	onNavigate?: (path: string) => void;
} = $props();

let email = $state('');
let password = $state('');
let nickname = $state('');
let loading = $state(false);
let error = $state<string | null>(null);

let emailPasswordEnabled = $state(true);
let oauthProviders = $state<OAuthProviderInfo[]>([]);

$effect(() => {
	getAuthProviders().then((resp) => {
		emailPasswordEnabled = resp.email_password;
		oauthProviders = resp.oauth;
	});
});

async function handleSubmit() {
	if (!email.trim() || !password || !nickname.trim()) return;
	loading = true;
	error = null;
	try {
		await authRegister(email.trim(), password, nickname.trim());
		onSuccess();
	} catch (e) {
		if (e instanceof Error) {
			try {
				const parsed = JSON.parse(e.message);
				error = parsed.error || e.message;
			} catch {
				error = e.message;
			}
		} else {
			error = 'Registration failed';
		}
	} finally {
		loading = false;
	}
}

const PROVIDER_ICONS: Record<string, string> = {
	github:
		'<path d="M12 0C5.37 0 0 5.37 0 12c0 5.31 3.435 9.795 8.205 11.385.6.105.825-.255.825-.57 0-.285-.015-1.23-.015-2.235-3.015.555-3.795-.735-4.035-1.41-.135-.345-.72-1.41-1.23-1.695-.42-.225-1.02-.78-.015-.795.945-.015 1.62.87 1.845 1.23 1.08 1.815 2.805 1.305 3.495.99.105-.78.42-1.305.765-1.605-2.67-.3-5.46-1.335-5.46-5.925 0-1.305.465-2.385 1.23-3.225-.12-.3-.54-1.53.12-3.18 0 0 1.005-.315 3.3 1.23.96-.27 1.98-.405 3-.405s2.04.135 3 .405c2.295-1.56 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.765.84 1.23 1.905 1.23 3.225 0 4.605-2.805 5.625-5.475 5.925.435.375.81 1.095.81 2.22 0 1.605-.015 2.895-.015 3.3 0 .315.225.69.825.57A12.02 12.02 0 0024 12c0-6.63-5.37-12-12-12z"/>',
	gitlab:
		'<path d="M23.6 9.593l-.033-.086L20.3.98a.851.851 0 00-.336-.405.878.878 0 00-1.002.056.878.878 0 00-.291.424l-2.209 6.776H7.538L5.329 1.055a.857.857 0 00-.291-.424.878.878 0 00-1.002-.056.851.851 0 00-.336.405L.433 9.502l-.033.09a6.013 6.013 0 001.996 6.954l.01.008.028.02 4.943 3.703 2.446 1.852 1.49 1.126a1.009 1.009 0 001.22 0l1.49-1.126 2.446-1.852 4.972-3.723.012-.01a6.018 6.018 0 001.992-6.951z"/>',
};
</script>

<svelte:head>
	<title>Register - opensession.io</title>
</svelte:head>

<div class="mx-auto max-w-sm pt-12">
	<h1 class="mb-6 text-center text-lg font-bold text-text-primary">Create Account</h1>

	{#if emailPasswordEnabled}
		<form onsubmit={(e) => { e.preventDefault(); handleSubmit(); }} class="space-y-3">
			<div>
				<label for="register-nickname" class="sr-only">Nickname</label>
				<input
					id="register-nickname"
					type="text"
					placeholder="Nickname"
					bind:value={nickname}
					class="w-full border border-border bg-bg-primary px-3 py-2 text-xs text-text-primary placeholder-text-muted outline-none focus:border-accent"
				/>
			</div>
			<div>
				<label for="register-email" class="sr-only">Email</label>
				<input
					id="register-email"
					type="email"
					placeholder="Email"
					bind:value={email}
					class="w-full border border-border bg-bg-primary px-3 py-2 text-xs text-text-primary placeholder-text-muted outline-none focus:border-accent"
				/>
			</div>
			<div>
				<label for="register-password" class="sr-only">Password</label>
				<input
					id="register-password"
					type="password"
					placeholder="Password (8-12 characters)"
					bind:value={password}
					class="w-full border border-border bg-bg-primary px-3 py-2 text-xs text-text-primary placeholder-text-muted outline-none focus:border-accent"
				/>
			</div>

			{#if error}
				<p class="text-xs text-error">{error}</p>
			{/if}

			<button
				type="submit"
				disabled={loading || !email.trim() || !password || !nickname.trim()}
				class="w-full bg-accent px-3 py-2 text-xs font-medium text-white hover:bg-accent/80 disabled:opacity-50"
			>
				{loading ? 'Creating account...' : 'Register'}
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
			class="mb-2 flex w-full items-center justify-center gap-2 border border-border bg-bg-secondary px-3 py-2 text-xs text-text-primary hover:bg-bg-hover"
		>
			{#if PROVIDER_ICONS[provider.id]}
				<svg class="h-4 w-4" viewBox="0 0 24 24" fill="currentColor">
					{@html PROVIDER_ICONS[provider.id]}
				</svg>
			{/if}
			Continue with {provider.display_name}
		</a>
	{/each}

	<div class="mt-4 text-center">
		<span class="text-xs text-text-muted">Already have an account? </span>
		<button
			onclick={() => onNavigate('/login')}
			class="text-xs text-accent hover:underline"
		>
			Sign In
		</button>
	</div>
</div>
