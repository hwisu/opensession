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
	getAuthProviders()
		.then((resp) => {
			emailPasswordEnabled = resp.email_password;
			oauthProviders = resp.oauth;
		})
		.catch(() => {
			emailPasswordEnabled = true;
			oauthProviders = [];
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
		error = e instanceof Error ? e.message : 'Registration failed';
	} finally {
		loading = false;
	}
}
</script>

<svelte:head>
	<title>Create Account - opensession.io</title>
</svelte:head>

<div class="mx-auto w-full max-w-sm px-3 py-10 sm:px-0">
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
					placeholder="Password"
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
			class="mb-2 block w-full border border-border bg-bg-secondary px-3 py-2 text-center text-xs text-text-primary hover:bg-bg-hover"
		>
			Continue with {provider.display_name}
		</a>
	{/each}

	<div class="mt-5 text-center">
		<span class="text-xs text-text-muted">Already registered? </span>
		<button
			type="button"
			onclick={() => onNavigate('/login')}
			class="text-xs text-accent hover:underline"
		>
			Sign In
		</button>
	</div>
</div>
