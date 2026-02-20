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

type CapabilityKey = Exclude<keyof CapabilitySnapshot, 'loaded' | 'error'>;

type FeatureCard = {
	id: string;
	flag: string;
	title: string;
	summary: string;
	outcomes: string[];
};

const featureCards: FeatureCard[] = [
	{
		id: 'sessions',
		flag: '/sessions, /session/{id}',
		title: 'Browse Sessions',
		summary:
			'Read public sessions quickly in list/detail views with shortcut-driven filtering and timeline review.',
		outcomes: [
			'List view scans many sessions fast',
			'Detail view tracks events, tool calls, and outcomes',
			'Same HAIL model in web and TUI',
		],
	},
	{
		id: 'git-share',
		flag: 'opensession publish upload <file> --git',
		title: 'Share via Git Branch',
		summary:
			'Session files can be committed to a git branch and shared as reproducible, reviewable artifacts.',
		outcomes: [
			'Branch history keeps session evolution visible',
			'Route-based preview can resolve branch/path inputs',
			'Sharing stays tool-agnostic through HAIL',
		],
	},
	{
		id: 'publish',
		flag: '/upload, opensession publish upload',
		title: 'Publish Sessions Online',
		summary:
			'Publish normalized sessions to the public feed so others can inspect real coding traces on the web.',
		outcomes: [
			'Upload API accepts HAIL sessions for hosting',
			'Public readers can discover sessions from `/sessions`',
			'Auth/API keys govern write paths in enabled runtimes',
		],
	},
];

const flowSteps = [
	{
		id: 'record',
		label: 'Record',
		detail: 'Capture AI coding activity in HAIL-compatible traces.',
	},
	{
		id: 'publish',
		label: 'Publish',
		detail: 'Send sessions to online feed or git branch for stable sharing.',
	},
	{
		id: 'review',
		label: 'Review',
		detail: 'Browse `/sessions` and inspect detail timelines to understand real workflows.',
	},
	{
		id: 'improve',
		label: 'Improve',
		detail: 'Use 공개 세션 knowledge to strengthen open models and open tooling quality.',
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

function capabilityEffect(key: CapabilityKey, enabled: boolean): string {
	if (key === 'auth_enabled') {
		return enabled
			? 'Account login, API key linking, and owner write protection are active.'
			: 'Guest-only browsing profile; account sign-in/write actions stay unavailable.';
	}
	if (key === 'upload_enabled') {
		return enabled
			? 'Online publishing is enabled via `/upload` and CLI upload endpoints.'
			: 'This runtime is read-only for online publish; browsing still works.';
	}
	if (key === 'ingest_preview_enabled') {
		return enabled
			? 'Parser preview/candidate selection is enabled before ingest.'
			: 'Advanced preview step is disabled; ingest path may still exist elsewhere.';
	}
	return enabled
		? 'Git branch/path route preview works under `/gh/...`.'
		: 'Git route preview is unavailable in this runtime profile.';
}

function runtimeProfileLabel(snapshot: CapabilitySnapshot): string {
	if (!snapshot.loaded) return 'Detecting runtime profile...';
	if (snapshot.upload_enabled || snapshot.ingest_preview_enabled || snapshot.gh_share_enabled) {
		return 'Capture + share profile';
	}
	if (snapshot.auth_enabled) return 'Read-only browse profile (auth enabled)';
	return 'Read-only browse profile';
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
				OpenSession helps teams browse sessions, share session artifacts through git branches, and publish
				sessions online. 목표는 공개 세션을 통해 open model 생태계에 실질적인 학습 신호를 제공하는 것입니다.
			</p>
			<div class="flex flex-wrap gap-2">
				<button
					type="button"
					onclick={() => onNavigate('/sessions')}
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
		<h2 class="mb-2 text-xl font-semibold text-text-primary">Runtime Capability Matrix (Operator View)</h2>
		<p class="mb-4 text-xs text-text-secondary">
			상단 Feature Map은 제품 목표를 설명하고, 아래 Matrix는 현재 배포 프로필의 운영 플래그를 보여줍니다.
		</p>
		<div class="mb-3 border border-border bg-bg-secondary px-3 py-2 text-xs text-text-secondary">
			{runtimeProfileLabel(capabilities)}
		</div>

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
							{capabilityEffect('auth_enabled', capabilities.auth_enabled)}
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
							{capabilityEffect('upload_enabled', capabilities.upload_enabled)}
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
							{capabilityEffect('ingest_preview_enabled', capabilities.ingest_preview_enabled)}
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
							{capabilityEffect('gh_share_enabled', capabilities.gh_share_enabled)}
						</td>
					</tr>
				</tbody>
			</table>
		</div>
		<p class="mt-3 text-[11px] text-text-muted">
			Flags are independent. Example: auth can be enabled while upload/preview/share are disabled in read-only
			worker deployments.
		</p>
	</section>
</div>
