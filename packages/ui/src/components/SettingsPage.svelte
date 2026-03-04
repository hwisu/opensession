<script lang="ts">
import {
	ApiError,
	createGitCredential,
	detectSummaryProvider,
	deleteGitCredential,
	getSettings,
	getRuntimeSettings,
	isAuthenticated,
	issueApiKey,
	listGitCredentials,
	updateRuntimeSettings,
} from '../api';
import type {
	DesktopRuntimeSettingsResponse,
	DesktopSummaryProviderDetectResponse,
	GitCredentialSummary,
	UserSettings,
} from '../types';

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
let credentials = $state<GitCredentialSummary[]>([]);
let credentialsLoading = $state(false);
let credentialsError = $state<string | null>(null);
let credentialsSupported = $state(true);
let creatingCredential = $state(false);
let deletingCredentialId = $state<string | null>(null);

let runtimeSettings = $state<DesktopRuntimeSettingsResponse | null>(null);
let runtimeLoading = $state(false);
let runtimeSaving = $state(false);
let runtimeDetecting = $state(false);
let runtimeSupported = $state(true);
let runtimeError = $state<string | null>(null);
let runtimeDetectMessage = $state<string | null>(null);
let runtimeSessionDefaultView = $state<'full' | 'compressed'>('full');
let runtimeProvider = $state('disabled');
let runtimeEndpoint = $state('');
let runtimeModel = $state('');
let runtimeSourceMode = $state('session_only');
let runtimeResponseStyle = $state('standard');
let runtimeOutputShape = $state('layered');
let runtimeOutputInstruction = $state('');
let runtimeTriggerMode = $state('on_session_save');
let runtimePersistMode = $state('local_db');
let runtimeTemplateSlotsText = $state('');

let credentialLabel = $state('');
let credentialHost = $state('');
let credentialPathPrefix = $state('');
let credentialHeaderName = $state('Authorization');
let credentialHeaderValue = $state('');

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
		await loadGitCredentials();
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

async function loadGitCredentials() {
	credentialsLoading = true;
	credentialsError = null;
	credentialsSupported = true;
	try {
		credentials = await listGitCredentials();
	} catch (err) {
		credentials = [];
		if (err instanceof ApiError && err.status === 404) {
			credentialsSupported = false;
			return;
		}
		credentialsError = normalizeError(err, 'Failed to load git credentials');
	} finally {
		credentialsLoading = false;
	}
}

function applyRuntimeSettingsToDraft(settings: DesktopRuntimeSettingsResponse) {
	runtimeSessionDefaultView =
		settings.session_default_view === 'compressed' ? 'compressed' : 'full';
	const summary =
		settings.summary && typeof settings.summary === 'object'
			? (settings.summary as Record<string, unknown>)
			: {};
	runtimeProvider = String(summary.provider ?? 'disabled');
	runtimeEndpoint = String(summary.endpoint ?? '');
	runtimeModel = String(summary.model ?? '');
	runtimeSourceMode = String(summary.source_mode ?? 'session_only');
	runtimeResponseStyle = String(summary.response_style ?? 'standard');
	runtimeOutputShape = String(summary.output_shape ?? 'layered');
	runtimeOutputInstruction = String(summary.output_instruction ?? '');
	runtimeTriggerMode = String(summary.trigger_mode ?? 'on_session_save');
	runtimePersistMode = String(summary.persist_mode ?? 'local_db');
	const templateSlots =
		summary.template_slots && typeof summary.template_slots === 'object'
			? (summary.template_slots as Record<string, unknown>)
			: {};
	runtimeTemplateSlotsText = Object.entries(templateSlots)
		.map(([key, value]) => `${key}=${String(value)}`)
		.join('\n');
}

async function loadRuntimeSettings() {
	runtimeLoading = true;
	runtimeError = null;
	runtimeSupported = true;
	try {
		const settings = await getRuntimeSettings();
		runtimeSettings = settings;
		applyRuntimeSettingsToDraft(settings);
	} catch (err) {
		runtimeSettings = null;
		if (err instanceof ApiError && err.status === 501) {
			runtimeSupported = false;
		} else {
			runtimeError = normalizeError(err, 'Failed to load runtime settings');
		}
	} finally {
		runtimeLoading = false;
	}
}

function parseTemplateSlots(text: string): Record<string, string> {
	const out: Record<string, string> = {};
	for (const line of text.split('\n')) {
		const trimmed = line.trim();
		if (!trimmed) continue;
		const idx = trimmed.indexOf('=');
		if (idx <= 0) continue;
		const key = trimmed.slice(0, idx).trim();
		const value = trimmed.slice(idx + 1).trim();
		if (!key) continue;
		out[key] = value;
	}
	return out;
}

async function handleSaveRuntimeSettings() {
	runtimeSaving = true;
	runtimeError = null;
	runtimeDetectMessage = null;
	try {
		const updated = await updateRuntimeSettings({
			session_default_view: runtimeSessionDefaultView,
			summary: {
				provider: runtimeProvider,
				endpoint: runtimeEndpoint,
				model: runtimeModel,
				source_mode: runtimeSourceMode,
				response_style: runtimeResponseStyle,
				output_shape: runtimeOutputShape,
				output_instruction: runtimeOutputInstruction,
				trigger_mode: runtimeTriggerMode,
				persist_mode: runtimePersistMode,
				template_slots: parseTemplateSlots(runtimeTemplateSlotsText),
			},
		});
		runtimeSettings = updated;
		applyRuntimeSettingsToDraft(updated);
		runtimeDetectMessage = 'Runtime settings saved.';
	} catch (err) {
		runtimeError = normalizeError(err, 'Failed to save runtime settings');
	} finally {
		runtimeSaving = false;
	}
}

async function handleDetectRuntimeProvider() {
	runtimeDetecting = true;
	runtimeDetectMessage = null;
	runtimeError = null;
	try {
		const detected: DesktopSummaryProviderDetectResponse = await detectSummaryProvider();
		if (!detected.detected || !detected.provider) {
			runtimeDetectMessage = 'No local provider detected.';
			return;
		}
		const updated = await updateRuntimeSettings({
			summary: {
				provider: detected.provider,
				model: detected.model ?? '',
				endpoint: detected.endpoint ?? '',
			},
		});
		runtimeSettings = updated;
		applyRuntimeSettingsToDraft(updated);
		runtimeDetectMessage = `Detected and applied provider: ${detected.provider}`;
	} catch (err) {
		runtimeError = normalizeError(err, 'Failed to detect/apply local provider');
	} finally {
		runtimeDetecting = false;
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

async function handleCreateCredential() {
	creatingCredential = true;
	credentialsError = null;
	try {
		await createGitCredential({
			label: credentialLabel,
			host: credentialHost,
			path_prefix: credentialPathPrefix.trim() ? credentialPathPrefix.trim() : null,
			header_name: credentialHeaderName,
			header_value: credentialHeaderValue,
		});
		credentialLabel = '';
		credentialHost = '';
		credentialPathPrefix = '';
		credentialHeaderName = 'Authorization';
		credentialHeaderValue = '';
		await loadGitCredentials();
	} catch (err) {
		credentialsError = normalizeError(err, 'Failed to save git credential');
	} finally {
		creatingCredential = false;
	}
}

async function handleDeleteCredential(id: string) {
	deletingCredentialId = id;
	credentialsError = null;
	try {
		await deleteGitCredential(id);
		await loadGitCredentials();
	} catch (err) {
		credentialsError = normalizeError(err, 'Failed to delete git credential');
	} finally {
		deletingCredentialId = null;
	}
}

$effect(() => {
	loadSettings();
	loadRuntimeSettings();
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

		<section class="border border-border bg-bg-secondary p-4" data-testid="git-credential-settings">
			<div class="space-y-1">
				<h2 class="text-sm font-semibold text-text-primary">Private Git Credentials</h2>
				<p class="text-xs text-text-secondary">
					Preferred: connect GitHub/GitLab OAuth. Manual credentials are used for private self-managed or generic git remotes.
				</p>
			</div>

			{#if !credentialsSupported}
				<p class="mt-3 text-xs text-text-muted">
					This deployment does not expose credential management endpoints.
				</p>
			{:else}
				<div class="mt-4 space-y-3">
					<div class="grid gap-2 sm:grid-cols-2">
						<input
							data-testid="git-credential-label"
							type="text"
							placeholder="Label"
							bind:value={credentialLabel}
							class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
						/>
						<input
							data-testid="git-credential-host"
							type="text"
							placeholder="Host (e.g. gitlab.internal.example.com)"
							bind:value={credentialHost}
							class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
						/>
						<input
							data-testid="git-credential-path-prefix"
							type="text"
							placeholder="Path prefix (optional, e.g. group/subgroup)"
							bind:value={credentialPathPrefix}
							class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
						/>
						<input
							data-testid="git-credential-header-name"
							type="text"
							placeholder="Header name"
							bind:value={credentialHeaderName}
							class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
						/>
						<input
							data-testid="git-credential-header-value"
							type="password"
							placeholder="Header value (secret)"
							bind:value={credentialHeaderValue}
							class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary sm:col-span-2"
						/>
					</div>
					<div class="flex justify-end">
						<button
							type="button"
							data-testid="git-credential-save"
							onclick={handleCreateCredential}
							disabled={creatingCredential}
							class="bg-accent px-3 py-2 text-xs font-semibold text-white hover:bg-accent/85 disabled:opacity-60"
						>
							{creatingCredential ? 'Saving...' : 'Save credential'}
						</button>
					</div>
				</div>

				<div class="mt-4 border border-border/70">
					<div class="grid grid-cols-[1.1fr_1fr_1fr_auto] gap-2 border-b border-border bg-bg-primary px-3 py-2 text-[11px] uppercase tracking-[0.08em] text-text-muted">
						<span>Label</span>
						<span>Host</span>
						<span>Path Prefix</span>
						<span>Action</span>
					</div>
					{#if credentialsLoading}
						<div class="px-3 py-3 text-xs text-text-muted">Loading credentials...</div>
					{:else if credentials.length === 0}
						<div class="px-3 py-3 text-xs text-text-muted">No manual credentials registered.</div>
					{:else}
						{#each credentials as credential}
							<div class="grid grid-cols-[1.1fr_1fr_1fr_auto] items-center gap-2 border-b border-border/60 px-3 py-2 text-xs">
								<div class="text-text-primary">{credential.label}</div>
								<div class="font-mono text-[11px] text-text-secondary">{credential.host}</div>
								<div class="font-mono text-[11px] text-text-secondary">{credential.path_prefix || '*'}</div>
								<button
									type="button"
									data-testid={'git-credential-delete-' + credential.id}
									disabled={deletingCredentialId === credential.id}
									onclick={() => handleDeleteCredential(credential.id)}
									class="border border-border px-2 py-1 text-[11px] text-text-secondary hover:text-text-primary disabled:opacity-60"
								>
									{deletingCredentialId === credential.id ? 'Deleting...' : 'Delete'}
								</button>
							</div>
						{/each}
					{/if}
				</div>
				<p class="mt-2 text-[11px] text-text-muted">
					Secrets are never shown again after save. Stored values are encrypted at rest.
				</p>
			{/if}
		</section>

		{#if error}
			<p class="text-xs text-error">{error}</p>
		{/if}
	{#if credentialsError}
		<p class="text-xs text-error">{credentialsError}</p>
	{/if}
	{/if}

	<section class="border border-border bg-bg-secondary p-4" data-testid="runtime-summary-settings">
		<div class="flex flex-wrap items-center justify-between gap-3">
			<div>
				<h2 class="text-sm font-semibold text-text-primary">Runtime Summary (Desktop)</h2>
				<p class="mt-1 text-xs text-text-secondary">
					Global default view, local summary provider, and output format controls.
				</p>
			</div>
			<div class="flex items-center gap-2">
				<button
					type="button"
					onclick={handleDetectRuntimeProvider}
					disabled={!runtimeSupported || runtimeDetecting || runtimeSaving || runtimeLoading}
					class="border border-border px-2 py-1 text-xs text-text-secondary hover:text-text-primary disabled:opacity-60"
				>
					{runtimeDetecting ? 'Detecting...' : 'Detect Provider'}
				</button>
				<button
					type="button"
					onclick={handleSaveRuntimeSettings}
					disabled={!runtimeSupported || runtimeSaving || runtimeLoading}
					class="bg-accent px-3 py-2 text-xs font-semibold text-white hover:bg-accent/85 disabled:opacity-60"
				>
					{runtimeSaving ? 'Saving...' : 'Save Runtime'}
				</button>
			</div>
		</div>

		{#if runtimeLoading}
			<p class="mt-3 text-xs text-text-muted">Loading runtime settings...</p>
		{:else if !runtimeSupported}
			<p class="mt-3 text-xs text-text-muted">
				Runtime settings are not available in this environment (desktop IPC required).
			</p>
		{:else}
			<div class="mt-4 grid gap-2 sm:grid-cols-2">
				<label class="text-xs text-text-secondary">
					<span class="mb-1 block">Default Session View</span>
					<select bind:value={runtimeSessionDefaultView} class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary">
						<option value="full">full</option>
						<option value="compressed">compressed</option>
					</select>
				</label>
				<label class="text-xs text-text-secondary">
					<span class="mb-1 block">Summary Provider</span>
					<select bind:value={runtimeProvider} class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary">
						<option value="disabled">disabled</option>
						<option value="ollama">ollama</option>
						<option value="codex_exec">codex_exec</option>
						<option value="claude_cli">claude_cli</option>
					</select>
				</label>
				<label class="text-xs text-text-secondary">
					<span class="mb-1 block">Provider Endpoint</span>
					<input bind:value={runtimeEndpoint} class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary" />
				</label>
				<label class="text-xs text-text-secondary">
					<span class="mb-1 block">Provider Model</span>
					<input bind:value={runtimeModel} class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary" />
				</label>
				<label class="text-xs text-text-secondary">
					<span class="mb-1 block">Source Mode</span>
					<select bind:value={runtimeSourceMode} class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary">
						<option value="session_only">session_only</option>
						<option value="session_or_git_changes">session_or_git_changes</option>
					</select>
				</label>
				<label class="text-xs text-text-secondary">
					<span class="mb-1 block">Response Style</span>
					<select bind:value={runtimeResponseStyle} class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary">
						<option value="compact">compact</option>
						<option value="standard">standard</option>
						<option value="detailed">detailed</option>
					</select>
				</label>
				<label class="text-xs text-text-secondary">
					<span class="mb-1 block">Output Shape</span>
					<select bind:value={runtimeOutputShape} class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary">
						<option value="layered">layered</option>
						<option value="file_list">file_list</option>
						<option value="security_first">security_first</option>
					</select>
				</label>
				<label class="text-xs text-text-secondary">
					<span class="mb-1 block">Trigger Mode</span>
					<select bind:value={runtimeTriggerMode} class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary">
						<option value="manual">manual</option>
						<option value="on_session_save">on_session_save</option>
					</select>
				</label>
				<label class="text-xs text-text-secondary">
					<span class="mb-1 block">Persist Mode</span>
					<select bind:value={runtimePersistMode} class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary">
						<option value="none">none</option>
						<option value="local_db">local_db</option>
					</select>
				</label>
				<label class="text-xs text-text-secondary sm:col-span-2">
					<span class="mb-1 block">Output Instruction</span>
					<textarea bind:value={runtimeOutputInstruction} rows="3" class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"></textarea>
				</label>
				<label class="text-xs text-text-secondary sm:col-span-2">
					<span class="mb-1 block">Template Slots (`key=value` per line)</span>
					<textarea bind:value={runtimeTemplateSlotsText} rows="4" class="w-full border border-border bg-bg-primary px-2 py-2 font-mono text-xs text-text-primary"></textarea>
				</label>
			</div>
		{/if}
		{#if runtimeError}
			<p class="mt-2 text-xs text-error">{runtimeError}</p>
		{/if}
		{#if runtimeDetectMessage}
			<p class="mt-2 text-xs text-text-secondary">{runtimeDetectMessage}</p>
		{/if}
	</section>
</div>
