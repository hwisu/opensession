<script lang="ts">
import {
	acceptInvitation,
	authLogout,
	changePassword,
	clearApiKey,
	declineInvitation,
	getAuthProviders,
	getSettings,
	isAuthenticated,
	linkOAuth,
	listInvitations,
	regenerateApiKey,
	setApiKey,
} from '../api';
import type { InvitationResponse, OAuthProviderInfo, UserSettings } from '../types';

const {
	onNavigate = (_path: string) => {},
}: {
	onNavigate?: (path: string) => void;
} = $props();

let settings = $state<UserSettings | null>(null);
let loading = $state(true);
let error = $state<string | null>(null);

let manualKey = $state('');
let showManualKeyInput = $state(false);
let showKey = $state(false);
let regenerating = $state(false);

// OAuth linking
let linkingProvider = $state<string | null>(null);
let oauthLinked = $state(false);
let oauthLinkError = $state<string | null>(null);
let availableProviders = $state<OAuthProviderInfo[]>([]);

// Invitations
let invitations = $state<InvitationResponse[]>([]);
let invitationLoading = $state<string | null>(null);
let invitationError = $state<string | null>(null);

// Password change
let currentPassword = $state('');
let newPassword = $state('');
let confirmPassword = $state('');
let changingPassword = $state(false);
let passwordError = $state<string | null>(null);
let passwordSuccess = $state(false);

type SettingsTab = 'account' | 'access' | 'invitations' | 'local_ops';
let activeTab = $state<SettingsTab>('account');

const settingsTabs = $derived.by(() => {
	const tabs: { id: SettingsTab; label: string }[] = [
		{ id: 'account', label: 'Account' },
		{ id: 'access', label: 'Access (API/OAuth)' },
	];
	if (invitations.length > 0) {
		tabs.push({ id: 'invitations', label: 'Invitations' });
	}
	tabs.push({ id: 'local_ops', label: 'Local Ops' });
	return tabs;
});

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

function handleSetKey() {
	if (manualKey.trim()) {
		setApiKey(manualKey.trim());
		manualKey = '';
		fetchSettings();
	}
}

async function handleLogout() {
	await authLogout();
	settings = null;
	manualKey = '';
	showManualKeyInput = false;
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

async function handleChangePassword() {
	if (!currentPassword || !newPassword) return;
	if (newPassword !== confirmPassword) {
		passwordError = 'Passwords do not match';
		return;
	}
	if (newPassword.length < 8) {
		passwordError = 'Password must be at least 8 characters';
		return;
	}
	if (newPassword.length > 12) {
		passwordError = 'Password must be at most 12 characters';
		return;
	}

	changingPassword = true;
	passwordError = null;
	passwordSuccess = false;
	try {
		await changePassword(currentPassword, newPassword);
		passwordSuccess = true;
		currentPassword = '';
		newPassword = '';
		confirmPassword = '';
	} catch (e) {
		if (e instanceof Error) {
			try {
				const parsed = JSON.parse(e.message);
				passwordError = parsed.error || e.message;
			} catch {
				passwordError = e.message;
			}
		} else {
			passwordError = 'Failed to change password';
		}
	} finally {
		changingPassword = false;
	}
}

async function handleLinkOAuth(providerId: string) {
	linkingProvider = providerId;
	oauthLinkError = null;
	try {
		await linkOAuth(providerId);
	} catch (e) {
		if (e instanceof Error) {
			try {
				const parsed = JSON.parse(e.message);
				oauthLinkError = parsed.error || e.message;
			} catch {
				oauthLinkError = e.message;
			}
		} else {
			oauthLinkError = `Failed to link provider`;
		}
		linkingProvider = null;
	}
}

async function handleAcceptInvitation(inv: InvitationResponse) {
	invitationLoading = inv.id;
	invitationError = null;
	try {
		const result = await acceptInvitation(inv.id);
		invitations = invitations.filter((i) => i.id !== inv.id);
		onNavigate(`/teams/${result.team_id}`);
	} catch (e) {
		if (e instanceof Error) {
			try {
				const parsed = JSON.parse(e.message);
				invitationError = parsed.error || e.message;
			} catch {
				invitationError = e.message;
			}
		} else {
			invitationError = 'Failed to accept invitation';
		}
	} finally {
		invitationLoading = null;
	}
}

async function handleDeclineInvitation(inv: InvitationResponse) {
	invitationLoading = inv.id;
	invitationError = null;
	try {
		await declineInvitation(inv.id);
		invitations = invitations.filter((i) => i.id !== inv.id);
	} catch (e) {
		invitationError = e instanceof Error ? e.message : 'Failed to decline invitation';
	} finally {
		invitationLoading = null;
	}
}

/** Providers available to link (not already linked). */
const unlinkableProviders = $derived(
	availableProviders.filter((p) => !settings?.oauth_providers?.some((lp) => lp.provider === p.id)),
);

$effect(() => {
	if (isAuthenticated()) {
		fetchSettings();
		getAuthProviders().then((resp) => {
			availableProviders = resp.oauth;
		});
		listInvitations()
			.then((resp) => {
				invitations = resp.invitations;
			})
			.catch(() => {
				invitations = [];
			});
	} else {
		loading = false;
	}

	// Handle query params from OAuth linking redirect
	if (typeof window !== 'undefined') {
		const params = new URLSearchParams(window.location.search);
		if (params.get('oauth_linked') === 'true') {
			oauthLinked = true;
			window.history.replaceState(null, '', window.location.pathname);
		}
		// Legacy param
		if (params.get('github_linked') === 'true') {
			oauthLinked = true;
			window.history.replaceState(null, '', window.location.pathname);
		}
		if (
			params.get('error') === 'oauth_already_linked' ||
			params.get('error') === 'github_already_linked'
		) {
			oauthLinkError = 'This account is already linked to another user';
			window.history.replaceState(null, '', window.location.pathname);
		}
	}
});

$effect(() => {
	const visibleTabs = settingsTabs.map((tab) => tab.id);
	if (!visibleTabs.includes(activeTab)) {
		activeTab = visibleTabs[0] ?? 'account';
	}
});
</script>

<svelte:head>
	<title>Settings - opensession.io</title>
</svelte:head>

<div class="mx-auto max-w-2xl">
	<h1 class="mb-4 text-lg font-bold text-text-primary">Settings</h1>

	{#if loading}
		<div class="py-8 text-center text-xs text-text-muted">Loading...</div>
	{:else if settings}
		<div class="space-y-4">
			<div class="flex flex-wrap gap-2 border border-border bg-bg-secondary p-2">
				{#each settingsTabs as tab}
					<button
						onclick={() => (activeTab = tab.id)}
						class={activeTab === tab.id
							? 'bg-accent px-2 py-1 text-xs font-medium text-white'
							: 'bg-bg-primary px-2 py-1 text-xs text-text-secondary hover:text-text-primary'}
					>
						{tab.label}
						{#if tab.id === 'invitations' && invitations.length > 0}
							<span class="ml-1 rounded bg-white/20 px-1">{invitations.length}</span>
						{/if}
					</button>
				{/each}
			</div>

			{#if activeTab === 'account'}
				<div class="border border-border bg-bg-secondary p-3">
					<h2 class="mb-2 text-sm font-medium text-text-primary">Account</h2>
					<div class="flex items-center justify-between">
						<div class="flex items-center gap-3">
							{#if settings.avatar_url}
								<img src={settings.avatar_url} alt="{settings.nickname} avatar" class="h-8 w-8 rounded-full" />
							{/if}
							<div>
								<p class="text-xs text-text-primary">{settings.nickname}</p>
								{#if settings.email}
									<p class="text-xs text-text-muted">{settings.email}</p>
								{/if}
							</div>
						</div>
						<button
							onclick={handleLogout}
							class="bg-bg-hover px-2 py-1 text-xs text-text-secondary hover:text-text-primary"
						>
							Logout
						</button>
					</div>
				</div>

				{#if settings.email}
					<div class="border border-border bg-bg-secondary p-3">
						<h2 class="mb-2 text-sm font-medium text-text-primary">Change Password</h2>
						<form onsubmit={(e) => { e.preventDefault(); handleChangePassword(); }} class="space-y-2">
							<label for="current-pw" class="sr-only">Current password</label>
							<input
								id="current-pw"
								type="password"
								placeholder="Current password"
								bind:value={currentPassword}
								class="w-full border border-border bg-bg-primary px-3 py-1.5 text-xs text-text-primary placeholder-text-muted outline-none focus:border-accent"
							/>
							<label for="new-pw" class="sr-only">New password</label>
							<input
								id="new-pw"
								type="password"
								placeholder="New password"
								bind:value={newPassword}
								class="w-full border border-border bg-bg-primary px-3 py-1.5 text-xs text-text-primary placeholder-text-muted outline-none focus:border-accent"
							/>
							<label for="confirm-pw" class="sr-only">Confirm new password</label>
							<input
								id="confirm-pw"
								type="password"
								placeholder="Confirm new password"
								bind:value={confirmPassword}
								class="w-full border border-border bg-bg-primary px-3 py-1.5 text-xs text-text-primary placeholder-text-muted outline-none focus:border-accent"
							/>
							{#if passwordError}
								<p class="text-xs text-error">{passwordError}</p>
							{/if}
							{#if passwordSuccess}
								<p class="text-xs text-success">Password changed successfully</p>
							{/if}
							<button
								type="submit"
								disabled={changingPassword || !currentPassword || !newPassword || !confirmPassword}
								class="bg-accent px-2 py-1 text-xs text-white hover:bg-accent/80 disabled:opacity-50"
							>
								{changingPassword ? 'Changing...' : 'Change Password'}
							</button>
						</form>
					</div>
				{/if}
			{/if}

			{#if activeTab === 'access'}
				<div class="border border-border bg-bg-secondary p-3">
					<h2 class="mb-2 text-sm font-medium text-text-primary">Access (API/OAuth)</h2>

					{#if settings.oauth_providers && settings.oauth_providers.length > 0}
						<div class="mb-3">
							<p class="mb-1.5 text-xs text-text-muted">Linked accounts</p>
							{#each settings.oauth_providers as lp}
								<p class="text-xs text-text-secondary">
									{lp.display_name}: <span class="text-text-primary">{lp.provider_username}</span>
								</p>
							{/each}
						</div>
					{/if}

					{#if unlinkableProviders.length > 0}
						<div class="mb-3 border-t border-border pt-3">
							{#each unlinkableProviders as provider}
								<button
									onclick={() => handleLinkOAuth(provider.id)}
									disabled={linkingProvider === provider.id}
									class="mr-2 flex items-center gap-2 bg-bg-hover px-3 py-1.5 text-xs text-text-secondary hover:text-text-primary disabled:opacity-50"
								>
									{linkingProvider === provider.id ? 'Redirecting...' : `Link ${provider.display_name}`}
								</button>
							{/each}
							{#if oauthLinkError}
								<p class="mt-1 text-xs text-error">{oauthLinkError}</p>
							{/if}
							{#if oauthLinked}
								<p class="mt-2 text-xs text-success">Account linked successfully!</p>
							{/if}
						</div>
					{/if}

					<div class="mb-2 border-t border-border pt-3">
						<p class="mb-2 text-xs text-text-muted">API Key</p>
						<div class="mb-2 flex items-center gap-2">
							<code class="flex-1 bg-bg-primary px-3 py-1.5 text-xs text-text-secondary">
								{showKey ? settings.api_key : '\u2022\u2022\u2022\u2022\u2022\u2022\u2022\u2022\u2022\u2022\u2022\u2022\u2022\u2022\u2022\u2022'}
							</code>
							<button
								onclick={() => (showKey = !showKey)}
								class="bg-bg-hover px-2 py-1.5 text-xs text-text-secondary hover:text-text-primary"
							>
								{showKey ? 'Hide' : 'Show'}
							</button>
							<button
								onclick={() => navigator.clipboard.writeText(settings!.api_key)}
								class="bg-bg-hover px-2 py-1.5 text-xs text-text-secondary hover:text-text-primary"
							>
								Copy
							</button>
						</div>
						<button
							onclick={handleRegenerateKey}
							disabled={regenerating}
							class="bg-error/10 px-2 py-1 text-xs text-error hover:bg-error/20 disabled:opacity-50"
						>
							{regenerating ? 'Regenerating...' : 'Regenerate Key'}
						</button>
					</div>
				</div>
			{/if}

			{#if activeTab === 'invitations'}
				<div class="border border-border bg-bg-secondary p-3">
					<h2 class="mb-2 text-sm font-medium text-text-primary">Invitations</h2>
					{#if invitationError}
						<p class="mb-2 text-xs text-error">{invitationError}</p>
					{/if}
					{#if invitations.length === 0}
						<p class="text-xs text-text-muted">No pending invitations.</p>
					{:else}
						<div class="space-y-2">
							{#each invitations as inv (inv.id)}
								<div class="flex items-center justify-between border border-border bg-bg-primary p-2">
									<div>
										<p class="text-xs font-medium text-text-primary">{inv.team_name}</p>
										<p class="text-xs text-text-muted">
											Invited by {inv.invited_by_nickname} as {inv.role}
										</p>
									</div>
									<div class="flex gap-2">
										<button
											onclick={() => handleAcceptInvitation(inv)}
											disabled={invitationLoading === inv.id}
											class="bg-accent px-2 py-1 text-xs text-white hover:bg-accent/80 disabled:opacity-50"
										>
											Accept
										</button>
										<button
											onclick={() => handleDeclineInvitation(inv)}
											disabled={invitationLoading === inv.id}
											class="bg-bg-hover px-2 py-1 text-xs text-text-secondary hover:text-text-primary disabled:opacity-50"
										>
											Decline
										</button>
									</div>
								</div>
							{/each}
						</div>
					{/if}
				</div>
			{/if}

			{#if activeTab === 'local_ops'}
				<div class="border border-border bg-bg-secondary p-3">
					<h2 class="mb-2 text-sm font-medium text-text-primary">Local Ops</h2>
					<p class="mb-2 text-xs text-text-secondary">
						Web cannot directly control local daemon settings. Use CLI/TUI with the same terms:
						`Publish Mode`, `Watchers`, `Stream Push`.
					</p>
					<div class="space-y-1 bg-bg-primary p-3 font-mono text-xs text-text-secondary">
						<p class="text-text-muted"># Open TUI</p>
						<p>opensession</p>
						<p class="mt-2 text-text-muted"># Start/stop daemon</p>
						<p>opensession daemon start --repo .</p>
						<p>opensession daemon stop</p>
						<p class="mt-2 text-text-muted"># Configure account key</p>
						<p>opensession account connect --api-key {showKey ? settings.api_key : '<your-api-key>'}</p>
					</div>
				</div>
			{/if}
		</div>
	{:else}
		<!-- Not authenticated -->
		<div class="space-y-4">
			<div class="border border-border bg-bg-secondary p-3">
				<h2 class="mb-2 text-sm font-medium text-text-primary">Not signed in</h2>
				<p class="mb-3 text-xs text-text-secondary">
					Sign in with your email or OAuth account to manage your settings.
				</p>
				<div class="flex gap-2">
					<button
						onclick={() => onNavigate('/login')}
						class="bg-accent px-3 py-1.5 text-xs font-medium text-white hover:bg-accent/80"
					>
						Sign In
					</button>
					<button
						onclick={() => onNavigate('/register')}
						class="bg-bg-hover px-3 py-1.5 text-xs text-text-secondary hover:text-text-primary"
					>
						Register
					</button>
				</div>
			</div>

			<div class="border border-border bg-bg-secondary p-3">
				<div class="flex items-center justify-between gap-2">
					<h2 class="text-sm font-medium text-text-primary">Use API key instead</h2>
					<button
						onclick={() => (showManualKeyInput = !showManualKeyInput)}
						class="bg-bg-hover px-2 py-1 text-xs text-text-secondary hover:text-text-primary"
					>
						{showManualKeyInput ? 'Hide' : 'Show'}
					</button>
				</div>
				{#if showManualKeyInput}
					<div class="mt-2 flex gap-2">
						<input
							type="text"
							placeholder="Paste your API key"
							bind:value={manualKey}
							onkeydown={(e) => e.key === 'Enter' && handleSetKey()}
							class="flex-1 border border-border bg-bg-primary px-3 py-1.5 text-xs text-text-primary placeholder-text-muted outline-none focus:border-accent"
						/>
						<button
							onclick={handleSetKey}
							disabled={!manualKey.trim()}
							class="bg-bg-hover px-3 py-1.5 text-xs text-text-secondary hover:text-text-primary disabled:opacity-50"
						>
							Set Key
						</button>
					</div>
				{/if}
			</div>
		</div>
	{/if}

	{#if error}
		<div class="mt-3 border border-error/30 bg-error/10 px-3 py-2 text-xs text-error">
			{error}
		</div>
	{/if}
</div>
