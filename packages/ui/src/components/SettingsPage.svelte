<script lang="ts">
import {
	ApiError,
	createGitCredential,
	detectSummaryProvider,
	deleteGitCredential,
	getApiCapabilities,
	getSettings,
	getRuntimeSettings,
	isAuthenticated,
	issueApiKey,
	listGitCredentials,
	updateRuntimeSettings,
	vectorIndexRebuild,
	vectorIndexStatus,
	vectorInstallModel,
	vectorPreflight,
} from '../api';
import type {
	DesktopChangeReaderScope,
	DesktopRuntimeSettingsResponse,
	DesktopSummaryOutputShape,
	DesktopSummaryProviderId,
	DesktopSummaryProviderDetectResponse,
	DesktopSummaryProviderTransport,
	DesktopSummaryResponseStyle,
	DesktopSummarySourceMode,
	DesktopSummaryStorageBackend,
	DesktopSummaryTriggerMode,
	DesktopVectorIndexStatusResponse,
	DesktopVectorPreflightResponse,
	DesktopVectorSearchGranularity,
	DesktopVectorSearchProvider,
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
let authApiEnabled = $state(true);
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
let runtimeProvider = $state<DesktopSummaryProviderId>('disabled');
let runtimeProviderTransport = $state<DesktopSummaryProviderTransport>('none');
let runtimeEndpoint = $state('');
let runtimeModel = $state('');
let runtimeSourceMode = $state<DesktopSummarySourceMode>('session_only');
let runtimeResponseStyle = $state<DesktopSummaryResponseStyle>('standard');
let runtimeOutputShape = $state<DesktopSummaryOutputShape>('layered');
let runtimePromptTemplate = $state('');
let runtimePromptDefaultTemplate = $state('');
let runtimeTriggerMode = $state<DesktopSummaryTriggerMode>('on_session_save');
let runtimeStorageBackend = $state<DesktopSummaryStorageBackend>('hidden_ref');
let runtimeVectorEnabled = $state(false);
let runtimeVectorProvider = $state<DesktopVectorSearchProvider>('ollama');
let runtimeVectorModel = $state('bge-m3');
let runtimeVectorEndpoint = $state('http://127.0.0.1:11434');
let runtimeVectorGranularity = $state<DesktopVectorSearchGranularity>('event_line_chunk');
let runtimeVectorChunkSizeLines = $state(12);
let runtimeVectorChunkOverlapLines = $state(3);
let runtimeVectorTopKChunks = $state(30);
let runtimeVectorTopKSessions = $state(20);
let runtimeChangeReaderEnabled = $state(false);
let runtimeChangeReaderScope = $state<DesktopChangeReaderScope>('summary_only');
let runtimeChangeReaderQaEnabled = $state(true);
let runtimeChangeReaderMaxContextChars = $state(12000);
let runtimeVectorPreflight = $state<DesktopVectorPreflightResponse | null>(null);
let runtimeVectorIndex = $state<DesktopVectorIndexStatusResponse | null>(null);
let runtimeVectorInstalling = $state(false);
let runtimeVectorReindexing = $state(false);

function providerTransportForId(id: DesktopSummaryProviderId): DesktopSummaryProviderTransport {
	if (id === 'ollama') return 'http';
	if (id === 'codex_exec' || id === 'claude_cli') return 'cli';
	return 'none';
}

function currentRuntimeProviderTransport(): DesktopSummaryProviderTransport {
	if (runtimeProvider === 'disabled') return 'none';
	return providerTransportForId(runtimeProvider);
}

function responsePreview(style: DesktopSummaryResponseStyle, shape: DesktopSummaryOutputShape): string {
	const changesPrefix =
		style === 'compact'
			? 'Updated session summary pipeline.'
			: style === 'detailed'
				? 'Refactored the desktop summary pipeline, split provider/prompt/response/storage concerns, and updated hidden-ref persistence semantics.'
				: 'Updated desktop summary pipeline with clearer runtime settings.';
	const security =
		shape === 'security_first'
			? 'Credential paths were isolated and storage policy now defaults to hidden_ref.'
			: 'none detected';

	const files =
		shape === 'file_list'
			? ['desktop/src-tauri/src/main.rs', 'packages/ui/src/components/SettingsPage.svelte']
			: ['desktop/src-tauri/src/main.rs'];
	const layer = shape === 'file_list' ? 'application' : 'presentation';

	return JSON.stringify(
		{
			changes: changesPrefix,
			auth_security: security,
			layer_file_changes: [
				{
					layer,
					summary: style === 'compact' ? 'Settings/runtime summary flow updated.' : 'Runtime settings and summary persistence behavior were updated.',
					files,
				},
			],
		},
		null,
		2,
	);
}

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

function delay(ms: number): Promise<void> {
	return new Promise((resolve) => {
		window.setTimeout(resolve, ms);
	});
}

async function loadSettings() {
	loading = true;
	error = null;
	authRequired = false;
	try {
		const capabilities = await getApiCapabilities();
		authApiEnabled = capabilities.auth_enabled;
	} catch {
		authApiEnabled = false;
	}

	if (!authApiEnabled) {
		settings = null;
		credentials = [];
		loading = false;
		return;
	}

	if (!isAuthenticated()) {
		authRequired = true;
		loading = false;
		return;
	}

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
	runtimeProvider = settings.summary.provider.id;
	runtimeProviderTransport = settings.summary.provider.transport;
	runtimeEndpoint = settings.summary.provider.endpoint ?? '';
	runtimeModel = settings.summary.provider.model ?? '';
	runtimeSourceMode = settings.ui_constraints.source_mode_locked
		? settings.ui_constraints.source_mode_locked_value
		: (settings.summary.source_mode ?? 'session_only');
	runtimeResponseStyle = settings.summary.response.style ?? 'standard';
	runtimeOutputShape = settings.summary.response.shape ?? 'layered';
	runtimePromptTemplate = settings.summary.prompt.template ?? '';
	runtimePromptDefaultTemplate = settings.summary.prompt.default_template ?? '';
	runtimeTriggerMode = settings.summary.storage.trigger ?? 'on_session_save';
	runtimeStorageBackend = settings.summary.storage.backend ?? 'hidden_ref';
	runtimeVectorEnabled = settings.vector_search.enabled ?? false;
	runtimeVectorProvider = settings.vector_search.provider ?? 'ollama';
	runtimeVectorModel = settings.vector_search.model ?? 'bge-m3';
	runtimeVectorEndpoint = settings.vector_search.endpoint ?? 'http://127.0.0.1:11434';
	runtimeVectorGranularity = settings.vector_search.granularity ?? 'event_line_chunk';
	runtimeVectorChunkSizeLines = settings.vector_search.chunk_size_lines ?? 12;
	runtimeVectorChunkOverlapLines = settings.vector_search.chunk_overlap_lines ?? 3;
	runtimeVectorTopKChunks = settings.vector_search.top_k_chunks ?? 30;
	runtimeVectorTopKSessions = settings.vector_search.top_k_sessions ?? 20;
	runtimeChangeReaderEnabled = settings.change_reader?.enabled ?? false;
	runtimeChangeReaderScope = settings.change_reader?.scope ?? 'summary_only';
	runtimeChangeReaderQaEnabled = settings.change_reader?.qa_enabled ?? true;
	runtimeChangeReaderMaxContextChars = settings.change_reader?.max_context_chars ?? 12000;
}

async function loadRuntimeSettings() {
	runtimeLoading = true;
	runtimeError = null;
	runtimeSupported = true;
	try {
		const settings = await getRuntimeSettings();
		runtimeSettings = settings;
		applyRuntimeSettingsToDraft(settings);
		await refreshVectorPreflight();
		await refreshVectorIndexStatus();
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

function buildRuntimeSummaryPayload() {
	return {
		provider: {
			id: runtimeProvider,
			endpoint: runtimeEndpoint,
			model: runtimeModel,
		},
		prompt: {
			template: runtimePromptTemplate,
		},
		response: {
			style: runtimeResponseStyle,
			shape: runtimeOutputShape,
		},
		storage: {
			trigger: runtimeTriggerMode,
			backend: runtimeStorageBackend,
		},
		source_mode: runtimeSourceMode,
	};
}

function buildRuntimeVectorPayload() {
	return {
		enabled: runtimeVectorEnabled,
		provider: runtimeVectorProvider,
		model: runtimeVectorModel,
		endpoint: runtimeVectorEndpoint,
		granularity: runtimeVectorGranularity,
		chunk_size_lines: runtimeVectorChunkSizeLines,
		chunk_overlap_lines: runtimeVectorChunkOverlapLines,
		top_k_chunks: runtimeVectorTopKChunks,
		top_k_sessions: runtimeVectorTopKSessions,
	};
}

function buildRuntimeChangeReaderPayload() {
	return {
		enabled: runtimeChangeReaderEnabled,
		scope: runtimeChangeReaderScope,
		qa_enabled: runtimeChangeReaderQaEnabled,
		max_context_chars: runtimeChangeReaderMaxContextChars,
	};
}

function handleRuntimeProviderChange() {
	runtimeProviderTransport = currentRuntimeProviderTransport();
	if (runtimeProviderTransport !== 'http') {
		runtimeEndpoint = '';
	}
}

function handleResetPromptTemplate() {
	runtimePromptTemplate = runtimePromptDefaultTemplate;
}

async function handleSaveRuntimeSettings() {
	runtimeSaving = true;
	runtimeError = null;
	runtimeDetectMessage = null;
	try {
		const updated = await updateRuntimeSettings({
			session_default_view: runtimeSessionDefaultView,
			summary: buildRuntimeSummaryPayload(),
			vector_search: buildRuntimeVectorPayload(),
			change_reader: buildRuntimeChangeReaderPayload(),
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
		runtimeProvider = detected.provider;
		runtimeProviderTransport =
			detected.transport ?? providerTransportForId(detected.provider ?? 'disabled');
		if (detected.model != null) runtimeModel = detected.model;
		if (detected.endpoint != null) runtimeEndpoint = detected.endpoint;
		const updated = await updateRuntimeSettings({
			summary: buildRuntimeSummaryPayload(),
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

async function refreshVectorPreflight() {
	try {
		runtimeVectorPreflight = await vectorPreflight();
		if (runtimeVectorPreflight.model_installed && runtimeVectorEnabled) {
			runtimeDetectMessage = 'Vector model is ready.';
		}
	} catch (err) {
		runtimeVectorPreflight = null;
		runtimeError = normalizeError(err, 'Failed to fetch vector model status');
	}
}

async function refreshVectorIndexStatus() {
	try {
		runtimeVectorIndex = await vectorIndexStatus();
	} catch (err) {
		runtimeVectorIndex = null;
		runtimeError = normalizeError(err, 'Failed to fetch vector index status');
	}
}

async function handleVectorInstallModel() {
	runtimeVectorInstalling = true;
	runtimeError = null;
	try {
		await vectorInstallModel(runtimeVectorModel);
		for (let attempt = 0; attempt < 120; attempt += 1) {
			await delay(1000);
			await refreshVectorPreflight();
			if (
				runtimeVectorPreflight &&
				runtimeVectorPreflight.install_state !== 'installing'
			) {
				break;
			}
		}
	} catch (err) {
		runtimeError = normalizeError(err, 'Failed to install vector model');
	} finally {
		runtimeVectorInstalling = false;
	}
}

async function handleVectorReindex() {
	runtimeVectorReindexing = true;
	runtimeError = null;
	try {
		await vectorIndexRebuild();
		for (let attempt = 0; attempt < 600; attempt += 1) {
			await delay(500);
			await refreshVectorIndexStatus();
			if (
				runtimeVectorIndex &&
				runtimeVectorIndex.state !== 'running'
			) {
				break;
			}
		}
	} catch (err) {
		runtimeError = normalizeError(err, 'Failed to start vector index rebuild');
	} finally {
		runtimeVectorReindexing = false;
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
	if (runtimeVectorEnabled && runtimeVectorPreflight && !runtimeVectorPreflight.model_installed) {
		runtimeVectorEnabled = false;
	}
});

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
		{:else if authApiEnabled && authRequired}
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
	{:else if authApiEnabled}
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
					Provider, prompt, response, and storage settings for desktop local runtime.
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
			<div class="mt-4 space-y-4">
				<label class="block text-xs text-text-secondary">
					<span class="mb-1 block">Default Session View</span>
					<select bind:value={runtimeSessionDefaultView} class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary">
						<option value="full">full</option>
						<option value="compressed">compressed</option>
					</select>
				</label>

				<section class="space-y-2 border border-border/60 p-3" data-testid="settings-runtime-provider">
					<h3 class="text-xs font-semibold uppercase tracking-[0.08em] text-text-muted">Provider</h3>
					<label class="block text-xs text-text-secondary">
						<span class="mb-1 block">Summary Provider</span>
						<select
							bind:value={runtimeProvider}
							onchange={handleRuntimeProviderChange}
							data-testid="runtime-provider-select"
							class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
						>
							<option value="disabled">disabled</option>
							<option value="ollama">ollama</option>
							<option value="codex_exec">codex_exec</option>
							<option value="claude_cli">claude_cli</option>
						</select>
					</label>
					<p class="text-[11px] text-text-muted" data-testid="runtime-provider-transport">
						transport: {currentRuntimeProviderTransport()}
					</p>
					{#if currentRuntimeProviderTransport() === 'http'}
						<label class="block text-xs text-text-secondary">
							<span class="mb-1 block">Endpoint</span>
							<input
								bind:value={runtimeEndpoint}
								data-testid="runtime-provider-endpoint"
								class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
							/>
						</label>
						<label class="block text-xs text-text-secondary">
							<span class="mb-1 block">Model</span>
							<input
								bind:value={runtimeModel}
								data-testid="runtime-provider-model"
								class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
							/>
						</label>
					{:else if currentRuntimeProviderTransport() === 'cli'}
						<label class="block text-xs text-text-secondary">
							<span class="mb-1 block">Model (optional)</span>
							<input
								bind:value={runtimeModel}
								data-testid="runtime-provider-model"
								class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
							/>
						</label>
						<p class="text-[11px] text-text-muted" data-testid="runtime-provider-cli-status">
							{runtimeDetectMessage ?? 'Run Detect Provider to verify CLI availability.'}
						</p>
					{/if}
				</section>

				<section class="space-y-2 border border-border/60 p-3" data-testid="settings-runtime-prompt">
					<h3 class="text-xs font-semibold uppercase tracking-[0.08em] text-text-muted">Prompt</h3>
					<label class="block text-xs text-text-secondary">
						<span class="mb-1 block">Prompt Template</span>
						<textarea
							bind:value={runtimePromptTemplate}
							data-testid="runtime-prompt-template"
							rows="6"
							class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
						></textarea>
					</label>
					<div class="flex justify-end">
						<button
							type="button"
							onclick={handleResetPromptTemplate}
							data-testid="runtime-prompt-reset-default"
							class="border border-border px-2 py-1 text-xs text-text-secondary hover:text-text-primary"
						>
							Reset to default
						</button>
					</div>
					<label class="block text-xs text-text-secondary">
						<span class="mb-1 block">Default Template (read-only)</span>
						<textarea
							readonly
							value={runtimePromptDefaultTemplate}
							rows="5"
							class="w-full border border-border/70 bg-bg-primary/60 px-2 py-2 text-xs text-text-muted"
						></textarea>
					</label>
				</section>

				<section class="space-y-2 border border-border/60 p-3" data-testid="settings-runtime-response">
					<h3 class="text-xs font-semibold uppercase tracking-[0.08em] text-text-muted">Response</h3>
					<div class="grid gap-2 sm:grid-cols-2">
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
					</div>
					<p class="text-[11px] text-text-muted">
						Desktop source mode is locked to <code>session_only</code> ({runtimeSourceMode}).
					</p>
					<div class="border border-border/70 bg-bg-primary p-2" data-testid="settings-response-preview">
						<p class="mb-2 text-[11px] uppercase tracking-[0.08em] text-text-muted">Response Preview</p>
						<pre class="overflow-x-auto text-xs text-text-secondary">{responsePreview(runtimeResponseStyle, runtimeOutputShape)}</pre>
					</div>
				</section>

				<section class="space-y-2 border border-border/60 p-3" data-testid="settings-runtime-vector">
					<h3 class="text-xs font-semibold uppercase tracking-[0.08em] text-text-muted">Vector Search</h3>
					<div class="grid gap-2 sm:grid-cols-2">
						<label class="text-xs text-text-secondary">
							<span class="mb-1 block">Model</span>
							<input
								bind:value={runtimeVectorModel}
								data-testid="runtime-vector-model"
								class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
							/>
						</label>
						<label class="text-xs text-text-secondary">
							<span class="mb-1 block">Endpoint</span>
							<input
								bind:value={runtimeVectorEndpoint}
								data-testid="runtime-vector-endpoint"
								class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
							/>
						</label>
						<label class="text-xs text-text-secondary">
							<span class="mb-1 block">Chunk Size (lines)</span>
							<input
								type="number"
								min="1"
								bind:value={runtimeVectorChunkSizeLines}
								class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
							/>
						</label>
						<label class="text-xs text-text-secondary">
							<span class="mb-1 block">Chunk Overlap (lines)</span>
							<input
								type="number"
								min="0"
								bind:value={runtimeVectorChunkOverlapLines}
								class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
							/>
						</label>
						<label class="text-xs text-text-secondary">
							<span class="mb-1 block">Top K Chunks</span>
							<input
								type="number"
								min="1"
								bind:value={runtimeVectorTopKChunks}
								class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
							/>
						</label>
						<label class="text-xs text-text-secondary">
							<span class="mb-1 block">Top K Sessions</span>
							<input
								type="number"
								min="1"
								bind:value={runtimeVectorTopKSessions}
								class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
							/>
						</label>
					</div>

					<label class="flex items-center gap-2 text-xs text-text-secondary">
						<input
							type="checkbox"
							bind:checked={runtimeVectorEnabled}
							disabled={!runtimeVectorPreflight?.model_installed}
							data-testid="runtime-vector-enable"
						/>
						<span>Enable semantic vector search</span>
					</label>

					<div class="flex flex-wrap items-center gap-2">
						<button
							type="button"
							data-testid="runtime-vector-install"
							onclick={handleVectorInstallModel}
							disabled={runtimeVectorInstalling || runtimeSaving || runtimeLoading}
							class="border border-border px-2 py-1 text-xs text-text-secondary hover:text-text-primary disabled:opacity-60"
						>
							{runtimeVectorInstalling ? 'Installing...' : 'Install model'}
						</button>
						<button
							type="button"
							data-testid="runtime-vector-reindex"
							onclick={handleVectorReindex}
							disabled={runtimeVectorReindexing || runtimeSaving || runtimeLoading}
							class="border border-border px-2 py-1 text-xs text-text-secondary hover:text-text-primary disabled:opacity-60"
						>
							{runtimeVectorReindexing ? 'Reindexing...' : 'Rebuild index'}
						</button>
					</div>

					<div class="rounded border border-border/60 bg-bg-primary px-2 py-2 text-[11px] text-text-muted" data-testid="runtime-vector-status">
						<p>provider: {runtimeVectorProvider} | granularity: {runtimeVectorGranularity}</p>
						{#if runtimeVectorPreflight}
							<p>
								model: {runtimeVectorPreflight.model} | reachable: {runtimeVectorPreflight.ollama_reachable ? 'yes' : 'no'} | installed:
								{runtimeVectorPreflight.model_installed ? 'yes' : 'no'} | install_state: {runtimeVectorPreflight.install_state}
								({runtimeVectorPreflight.progress_pct}%)
							</p>
							{#if runtimeVectorPreflight.message}
								<p class="mt-1">{runtimeVectorPreflight.message}</p>
							{/if}
						{:else}
							<p>vector model status unavailable.</p>
						{/if}
						{#if runtimeVectorIndex}
							<p class="mt-1">
								index_state: {runtimeVectorIndex.state} | processed: {runtimeVectorIndex.processed_sessions}/{runtimeVectorIndex.total_sessions}
							</p>
							{#if runtimeVectorIndex.message}
								<p class="mt-1">{runtimeVectorIndex.message}</p>
							{/if}
						{/if}
					</div>
				</section>

				<section class="space-y-2 border border-border/60 p-3" data-testid="settings-runtime-change-reader">
					<h3 class="text-xs font-semibold uppercase tracking-[0.08em] text-text-muted">Change Reader</h3>
					<label class="flex items-center gap-2 text-xs text-text-secondary">
						<input
							type="checkbox"
							bind:checked={runtimeChangeReaderEnabled}
							data-testid="runtime-change-reader-enable"
						/>
						<span>Enable notebook-style change reading</span>
					</label>
					<div class="grid gap-2 sm:grid-cols-2">
						<label class="text-xs text-text-secondary">
							<span class="mb-1 block">Default Scope</span>
							<select bind:value={runtimeChangeReaderScope} class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary">
								<option value="summary_only">summary_only</option>
								<option value="full_context">full_context</option>
							</select>
						</label>
						<label class="text-xs text-text-secondary">
							<span class="mb-1 block">Max Context Chars</span>
							<input
								type="number"
								min="1"
								bind:value={runtimeChangeReaderMaxContextChars}
								data-testid="runtime-change-reader-max-context"
								class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary"
							/>
						</label>
					</div>
					<label class="flex items-center gap-2 text-xs text-text-secondary">
						<input
							type="checkbox"
							bind:checked={runtimeChangeReaderQaEnabled}
							data-testid="runtime-change-reader-qa"
						/>
						<span>Allow Q&A about change details</span>
					</label>
					<p class="text-[11px] text-text-muted">
						Uses the configured summary provider when available, then falls back to local heuristic context extraction.
					</p>
				</section>

				<section class="space-y-2 border border-border/60 p-3" data-testid="settings-runtime-storage">
					<h3 class="text-xs font-semibold uppercase tracking-[0.08em] text-text-muted">Storage</h3>
					<div class="grid gap-2 sm:grid-cols-2">
						<label class="text-xs text-text-secondary">
							<span class="mb-1 block">Trigger</span>
							<select bind:value={runtimeTriggerMode} class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary">
								<option value="manual">manual</option>
								<option value="on_session_save">on_session_save</option>
							</select>
						</label>
						<label class="text-xs text-text-secondary">
							<span class="mb-1 block">Backend</span>
							<select bind:value={runtimeStorageBackend} class="w-full border border-border bg-bg-primary px-2 py-2 text-xs text-text-primary">
								<option value="hidden_ref">hidden_ref</option>
								<option value="local_db">local_db</option>
								<option value="none">none</option>
							</select>
						</label>
					</div>
					{#if runtimeStorageBackend === 'hidden_ref'}
						<p class="text-[11px] text-text-muted" data-testid="runtime-storage-hidden-ref-notice">
							<code>hidden_ref</code> stores summary artifacts in git-native refs. Search/filter metadata is still indexed in local SQLite
							(<code>OPENSESSION_LOCAL_DB_PATH</code> or default <code>~/.local/share/opensession/local.db</code>) for fast queries.
						</p>
					{/if}
				</section>
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
