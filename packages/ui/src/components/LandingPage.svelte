<script lang="ts">
import { onMount } from 'svelte';
import { getApiCapabilities } from '../api';

const {
	onNavigate = (_path: string) => {},
}: {
	onNavigate?: (path: string) => void;
} = $props();

type CapabilitySnapshot = {
	loaded: boolean;
	auth_enabled: boolean;
	upload_enabled: boolean;
	ingest_preview_enabled: boolean;
	gh_share_enabled: boolean;
	error: string | null;
};

type FeatureCard = {
	id: string;
	flag: string;
	title: string;
	summary: string;
	outcomes: string[];
};

const featureCards: FeatureCard[] = [
	{
		id: 'capture',
		flag: '/upload',
		title: 'Capture Sessions',
		summary:
			'Server runtime provides upload and ingest preview APIs to normalize raw tool exports into HAIL JSONL.',
		outcomes: [
			'UI upload and CLI upload target the same session schema',
			'Parser preview exposes candidate parsing before ingestion',
			'Read-only runtimes keep capture paths visible but disabled',
		],
	},
	{
		id: 'explore',
		flag: '/',
		title: 'Explore Sessions',
		summary: 'Session feed and timeline detail are available in both server and worker profiles.',
		outcomes: [
			'`List` is one chronological feed across sessions',
			'`Agents` groups by max active agents for parallelism visibility',
			'Shortcuts: `t` tool, `o` order, `r` range, `l` layout, `/` search',
		],
	},
	{
		id: 'share',
		flag: '/gh/{owner}/{repo}/{ref}/{path...}',
		title: 'GitHub Route Preview',
		summary: 'Route parameters load and parse source files directly when `gh_share_enabled` is true.',
		outcomes: [
			'Parser selection flow resolves ambiguous files',
			'View/filter/parser state is synced in URL query params',
			'Disabled runtime shows explicit unsupported state',
		],
	},
	{
		id: 'access',
		flag: '/login',
		title: 'Auth and Account',
		summary:
			'Auth availability follows runtime `auth_enabled`. Active auth runtimes provide account menu and CLI API key linking.',
		outcomes: [
			'Top-right handle dropdown exposes account info and logout',
			'Landing/docs remain accessible for guests',
			'API keys are issued once and used for CLI-to-server connectivity',
		],
	},
];

const flowSteps = [
	{
		id: 'input',
		label: 'Input',
		detail: 'Raw session files are provided via `/upload`, CLI upload, or `/gh/...` route input.',
	},
	{
		id: 'parse',
		label: 'Parse',
		detail: 'Parsers normalize source records into HAIL events and expose parser-selection fallbacks.',
	},
	{
		id: 'index',
		label: 'Index',
		detail: 'Session metadata is indexed for feed sorting, filtering, and timeline sidebars.',
	},
	{
		id: 'review',
		label: 'Review',
		detail: 'Users review list/detail views with keyboard shortcuts and event-level filtering.',
	},
];

let capabilities = $state<CapabilitySnapshot>({
	loaded: false,
	auth_enabled: false,
	upload_enabled: false,
	ingest_preview_enabled: false,
	gh_share_enabled: false,
	error: null,
});

onMount(() => {
	let cancelled = false;
	getApiCapabilities()
		.then((next) => {
			if (cancelled) return;
			capabilities = {
				loaded: true,
				auth_enabled: next.auth_enabled,
				upload_enabled: next.upload_enabled,
				ingest_preview_enabled: next.ingest_preview_enabled,
				gh_share_enabled: next.gh_share_enabled,
				error: null,
			};
		})
		.catch((error) => {
			if (cancelled) return;
			capabilities = {
				loaded: true,
				auth_enabled: false,
				upload_enabled: false,
				ingest_preview_enabled: false,
				gh_share_enabled: false,
				error: error instanceof Error ? error.message : 'Failed to load capabilities',
			};
		});

	return () => {
		cancelled = true;
	};
});

function capabilityStatus(enabled: boolean): string {
	return enabled ? 'Available' : 'Disabled';
}

function capabilityClass(enabled: boolean): string {
	return enabled ? 'text-success' : 'text-warning';
}
</script>

<svelte:head>
	<title>OpenSession - Capability-Aware Session Review</title>
</svelte:head>

<div class="mx-auto w-full max-w-6xl px-3 py-8 sm:px-6 sm:py-10">
	<section class="grid gap-6 border border-border bg-bg-secondary p-5 sm:p-6 lg:grid-cols-[1.1fr_0.9fr]">
		<div class="space-y-4">
			<p class="text-[11px] uppercase tracking-[0.16em] text-text-muted">open format • local + web</p>
			<h1 class="text-3xl font-bold leading-tight text-text-primary sm:text-4xl">
				Track real AI coding sessions with one consistent data model.
			</h1>
			<p class="max-w-xl text-sm leading-relaxed text-text-secondary">
				OpenSession normalizes tool logs to HAIL, then serves searchable feed and timeline detail across
				web and TUI. Runtime flags from <code>/api/capabilities</code> define what is available now.
			</p>
			<div class="flex flex-wrap gap-2">
				<button
					type="button"
					onclick={() => onNavigate('/')}
					class="bg-accent px-4 py-2 text-xs font-semibold text-white transition-colors hover:bg-accent/85"
				>
					Open Sessions
				</button>
				<button
					type="button"
					onclick={() => onNavigate('/docs')}
					class="border border-border px-4 py-2 text-xs font-semibold text-text-secondary transition-colors hover:border-accent hover:text-accent"
				>
					Open Docs
				</button>
			</div>
		</div>

		<div class="border border-border bg-bg-primary p-4">
			<p class="mb-3 text-xs font-semibold uppercase tracking-[0.12em] text-text-muted">
				What You Can Verify Now
			</p>
			<ul class="space-y-2">
				<li class="text-sm text-text-secondary">
					Capability matrix values come directly from live <code>/api/capabilities</code>.
				</li>
				<li class="text-sm text-text-secondary">
					Feature map items map to current routes and commands in this repository.
				</li>
				<li class="text-sm text-text-secondary">
					Disabled runtime features are rendered as disabled states, not hidden marketing claims.
				</li>
			</ul>
		</div>
	</section>

	<section data-contract-section="feature-map" class="mt-6 border border-border p-4 sm:p-5">
		<div class="mb-3 text-xs uppercase tracking-[0.12em] text-text-muted">$ feature-map</div>
		<h2 class="mb-4 text-xl font-semibold text-text-primary">Feature Map</h2>
		<div class="grid gap-3 md:grid-cols-2">
			{#each featureCards as card}
				<article class="border border-border bg-bg-secondary p-4" data-feature-id={card.id}>
					<div class="mb-2 flex items-center justify-between">
						<span class="text-xs text-accent">{card.flag}</span>
						<span class="text-[11px] uppercase text-text-muted">{card.id}</span>
					</div>
					<h3 class="text-sm font-semibold text-text-primary">{card.title}</h3>
					<p class="mt-2 text-xs leading-relaxed text-text-secondary">{card.summary}</p>
					<ul class="mt-3 space-y-1">
						{#each card.outcomes as outcome}
							<li class="text-xs text-text-muted">- {outcome}</li>
						{/each}
					</ul>
				</article>
			{/each}
		</div>
	</section>

	<section data-contract-section="data-flow" class="mt-6 border border-border p-4 sm:p-5">
		<div class="mb-3 text-xs uppercase tracking-[0.12em] text-text-muted">$ data-flow</div>
		<h2 class="mb-4 text-xl font-semibold text-text-primary">Data Flow</h2>
		<div class="grid gap-2 md:grid-cols-4">
			{#each flowSteps as step, idx}
				<div class="border border-border bg-bg-secondary p-3" data-flow-step={step.id}>
					<div class="text-[11px] uppercase tracking-[0.08em] text-text-muted">Step {idx + 1}</div>
					<div class="mt-1 text-sm font-semibold text-text-primary">{step.label}</div>
					<p class="mt-2 text-xs leading-relaxed text-text-secondary">{step.detail}</p>
				</div>
			{/each}
		</div>
	</section>

	<section data-contract-section="capability-matrix" class="mt-6 border border-border p-4 sm:p-5">
		<div class="mb-3 text-xs uppercase tracking-[0.12em] text-text-muted">$ capability-matrix</div>
		<h2 class="mb-4 text-xl font-semibold text-text-primary">Capability Matrix</h2>

		{#if capabilities.error}
			<div class="mb-3 border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">
				Runtime detection fallback: {capabilities.error}
			</div>
		{/if}

		<div class="overflow-x-auto">
			<table class="w-full border-collapse text-xs">
				<thead>
					<tr class="bg-bg-secondary text-left text-text-muted">
						<th class="border border-border px-2 py-2">Capability</th>
						<th class="border border-border px-2 py-2">Status</th>
						<th class="border border-border px-2 py-2">User-visible effect</th>
					</tr>
				</thead>
				<tbody>
					<tr data-capability-key="auth_enabled">
						<td class="border border-border px-2 py-2 text-text-secondary">Authentication</td>
						<td class="border border-border px-2 py-2">
							{#if capabilities.loaded}
								<span class={capabilityClass(capabilities.auth_enabled)}>
									{capabilityStatus(capabilities.auth_enabled)}
								</span>
							{:else}
								<span class="text-text-muted">Detecting...</span>
							{/if}
						</td>
						<td class="border border-border px-2 py-2 text-text-secondary">
							Login form and token-based user flows.
						</td>
					</tr>
					<tr data-capability-key="upload_enabled">
						<td class="border border-border px-2 py-2 text-text-secondary">Upload API</td>
						<td class="border border-border px-2 py-2">
							{#if capabilities.loaded}
								<span class={capabilityClass(capabilities.upload_enabled)}>
									{capabilityStatus(capabilities.upload_enabled)}
								</span>
							{:else}
								<span class="text-text-muted">Detecting...</span>
							{/if}
						</td>
						<td class="border border-border px-2 py-2 text-text-secondary">
							Upload page and publish actions.
						</td>
					</tr>
					<tr data-capability-key="ingest_preview_enabled">
						<td class="border border-border px-2 py-2 text-text-secondary">Ingest Preview</td>
						<td class="border border-border px-2 py-2">
							{#if capabilities.loaded}
								<span class={capabilityClass(capabilities.ingest_preview_enabled)}>
									{capabilityStatus(capabilities.ingest_preview_enabled)}
								</span>
							{:else}
								<span class="text-text-muted">Detecting...</span>
							{/if}
						</td>
						<td class="border border-border px-2 py-2 text-text-secondary">
							Parser preview and candidate selection.
						</td>
					</tr>
					<tr data-capability-key="gh_share_enabled">
						<td class="border border-border px-2 py-2 text-text-secondary">GitHub Share Preview</td>
						<td class="border border-border px-2 py-2">
							{#if capabilities.loaded}
								<span class={capabilityClass(capabilities.gh_share_enabled)}>
									{capabilityStatus(capabilities.gh_share_enabled)}
								</span>
							{:else}
								<span class="text-text-muted">Detecting...</span>
							{/if}
						</td>
						<td class="border border-border px-2 py-2 text-text-secondary">
							Route-based source preview under <code>/gh/...</code>.
						</td>
					</tr>
				</tbody>
			</table>
		</div>
	</section>
</div>
