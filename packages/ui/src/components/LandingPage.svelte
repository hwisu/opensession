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
		flag: '--capture',
		title: 'Capture and Normalize',
		summary: 'Upload raw exports and normalize to HAIL JSONL for consistent storage.',
		outcomes: [
			'Parser auto-detection with parser hint fallback',
			'Unified schema for cross-tool timeline rendering',
			'Server and worker deployments share the same docs surface',
		],
	},
	{
		id: 'explore',
		flag: '--explore',
		title: 'Explore Sessions',
		summary: 'Filter and inspect sessions by time range, tool, and in-session event search.',
		outcomes: [
			'Session list with keyboard-friendly navigation',
			'Timeline filters for unified/native views',
			'Detail sidebar with tool/model/session metadata',
		],
	},
	{
		id: 'share',
		flag: '--share',
		title: 'GitHub Source Preview',
		summary: 'Preview parseable session files from GitHub routes without local imports.',
		outcomes: [
			'Parser selection flow when confidence is ambiguous',
			'URL-synced view and filter state',
			'Read-only fallback message in limited deployments',
		],
	},
	{
		id: 'operate',
		flag: '--operate',
		title: 'Runtime-Aware UX',
		summary: 'Capability flags drive UI behavior for auth, upload, preview, and sharing.',
		outcomes: [
			'Guest-safe landing and docs access',
			'Upload gated by runtime capabilities',
			'Consistent behavior across server and worker profiles',
		],
	},
];

const flowSteps = [
	{
		id: 'input',
		label: 'Input',
		detail: 'Session files or GitHub route payloads enter the ingest pipeline.',
	},
	{
		id: 'parse',
		label: 'Parse',
		detail: 'Parser preview resolves source format and normalizes into HAIL.',
	},
	{
		id: 'index',
		label: 'Index',
		detail: 'Session metadata is indexed for list filters and timeline summaries.',
	},
	{
		id: 'review',
		label: 'Review',
		detail: 'Users inspect timelines, filter events, and drill into details.',
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
	<title>OpenSession - Capture and Review AI Sessions</title>
</svelte:head>

<div class="mx-auto w-full max-w-6xl px-3 py-8 sm:px-6 sm:py-10">
	<section class="grid gap-6 border border-border bg-bg-secondary p-5 sm:p-6 lg:grid-cols-[1.1fr_0.9fr]">
		<div class="space-y-4">
			<p class="text-[11px] uppercase tracking-[0.16em] text-text-muted">open format • runtime aware</p>
			<h1 class="text-3xl font-bold leading-tight text-text-primary sm:text-4xl">
				AI sessions become a reusable engineering asset.
			</h1>
			<p class="max-w-xl text-sm leading-relaxed text-text-secondary">
				OpenSession captures session traces in HAIL, visualizes event timelines, and adapts features
				by runtime capabilities without changing the frontend route layer.
			</p>
			<div class="flex flex-wrap gap-2">
				<button
					type="button"
					onclick={() => onNavigate('/login')}
					class="bg-accent px-4 py-2 text-xs font-semibold text-white transition-colors hover:bg-accent/85"
				>
					Sign In
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
			<p class="mb-3 text-xs font-semibold uppercase tracking-[0.12em] text-text-muted">Delivery Focus</p>
			<ul class="space-y-2">
				<li class="text-sm text-text-secondary">Feature visibility first, not only marketing copy.</li>
				<li class="text-sm text-text-secondary">Single docs source served as markdown and HTML.</li>
				<li class="text-sm text-text-secondary">Contract + snapshot checks to prevent silent content loss.</li>
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
