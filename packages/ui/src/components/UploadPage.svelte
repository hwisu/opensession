<script lang="ts">
import { ApiError, isAuthenticated, listTeams, uploadSession } from '../api';
import type { Session, TeamResponse } from '../types';
import { formatDuration, getToolConfig } from '../types';
import AuthGuideCard from './AuthGuideCard.svelte';

const {
	teamMode = 'dropdown',
	onSuccess,
	onNavigate = (path: string) => {
		if (typeof window !== 'undefined') window.location.href = path;
	},
}: {
	teamMode?: 'dropdown' | 'manual';
	onSuccess: (id: string) => void;
	onNavigate?: (path: string) => void;
} = $props();

let parsedSession = $state<Session | null>(null);
let parseError = $state<string | null>(null);
let uploading = $state(false);
let uploadError = $state<string | null>(null);
let unauthorized = $state(false);
let rawJson = $state('');
let dragover = $state(false);

// Team selection state
let teams = $state<TeamResponse[]>([]);
let selectedTeamId = $state('');
let teamsLoading = $state(false);
let manualTeamId = $state('');

const effectiveTeamId = $derived(teamMode === 'dropdown' ? selectedTeamId : manualTeamId);

function parseJson(text: string) {
	parseError = null;
	parsedSession = null;
	try {
		const data = JSON.parse(text);
		if (!data.version || !data.session_id || !data.agent || !data.events) {
			throw new Error(
				'Invalid session format: missing required fields (version, session_id, agent, events)',
			);
		}
		parsedSession = data as Session;
	} catch (e) {
		parseError = e instanceof Error ? e.message : 'Invalid JSON';
	}
}

function handleFileInput(e: globalThis.Event) {
	if (unauthorized) return;
	const input = e.target as HTMLInputElement;
	const file = input.files?.[0];
	if (!file) return;
	const reader = new FileReader();
	reader.onload = () => {
		rawJson = reader.result as string;
		parseJson(rawJson);
	};
	reader.readAsText(file);
}

function handleDrop(e: DragEvent) {
	e.preventDefault();
	dragover = false;
	if (unauthorized) return;
	const file = e.dataTransfer?.files[0];
	if (!file) return;
	const reader = new FileReader();
	reader.onload = () => {
		rawJson = reader.result as string;
		parseJson(rawJson);
	};
	reader.readAsText(file);
}

function handleDragOver(e: DragEvent) {
	e.preventDefault();
	if (unauthorized) return;
	dragover = true;
}

function handleDragLeave() {
	dragover = false;
}

function handlePasteInput() {
	if (unauthorized) return;
	if (rawJson.trim()) {
		parseJson(rawJson);
	}
}

async function handleUpload() {
	if (!parsedSession || !effectiveTeamId) return;
	uploading = true;
	uploadError = null;
	try {
		const result = await uploadSession(parsedSession, effectiveTeamId);
		onSuccess(result.id);
	} catch (e) {
		if (e instanceof ApiError && (e.status === 401 || e.status === 403)) {
			unauthorized = true;
			return;
		}
		uploadError = e instanceof Error ? e.message : 'Upload failed';
	} finally {
		uploading = false;
	}
}

const tool = $derived(parsedSession ? getToolConfig(parsedSession.agent.tool) : null);

$effect(() => {
	unauthorized = !isAuthenticated();
	if (teamMode === 'dropdown') {
		teamsLoading = true;
		listTeams()
			.then((res) => {
				teams = res.teams;
				if (teams.length > 0 && !selectedTeamId) {
					selectedTeamId = teams[0].id;
				}
			})
			.catch((e) => {
				if (e instanceof ApiError && (e.status === 401 || e.status === 403)) {
					unauthorized = true;
				}
				teams = [];
			})
			.finally(() => {
				teamsLoading = false;
			});
	}
});
</script>

<svelte:head>
	<title>Upload Session - opensession.io</title>
</svelte:head>

<div class="mx-auto max-w-2xl">
	<h1 class="mb-2 text-lg font-bold text-text-primary">Upload Session</h1>
	<p class="mb-4 text-sm text-text-secondary">
		Upload a HAIL session JSON file to share with the community
	</p>

	{#if unauthorized}
		<div class="mb-4">
			<AuthGuideCard
				title="Upload requires sign in"
				description="Sign in first, then choose a target team and upload your session JSON."
				{onNavigate}
			/>
		</div>
	{/if}

	<!-- Team selection (branching) -->
	<div class="mb-3" class:opacity-50={unauthorized}>
		<label class="mb-1 block text-xs text-text-secondary" for="team-select">
			Team
		</label>
		{#if teamMode === 'dropdown'}
			{#if teamsLoading}
				<p class="text-xs text-text-muted">Loading teams...</p>
			{:else if teams.length === 0}
				<div class="border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">
					You need to be a member of a team to upload sessions.
					<a href="/teams" class="underline">Create or join a team</a> first.
				</div>
			{:else}
				<select
					id="team-select"
					bind:value={selectedTeamId}
					disabled={unauthorized}
					class="w-full border border-border bg-bg-secondary px-3 py-1.5 text-xs text-text-primary outline-none focus:border-accent"
				>
					{#each teams as team}
						<option value={team.id}>{team.name}</option>
					{/each}
				</select>
			{/if}
		{:else}
			<input
				id="team-select"
				type="text"
				bind:value={manualTeamId}
				disabled={unauthorized}
				placeholder="Enter team ID"
				class="w-full border border-border bg-bg-secondary px-3 py-1.5 text-xs text-text-primary placeholder-text-muted outline-none focus:border-accent"
			/>
		{/if}
	</div>

	<!-- Drop zone -->
	<div
		role="button"
		tabindex="0"
		ondrop={handleDrop}
		ondragover={handleDragOver}
		ondragleave={handleDragLeave}
		onkeydown={(e) => e.key === 'Enter' && document.getElementById('file-input')?.click()}
		class="mb-3 flex cursor-pointer flex-col items-center justify-center border-2 border-dashed p-6 transition-colors {dragover
			? 'border-accent bg-accent/5'
			: 'border-border hover:border-border-light'}"
		aria-disabled={unauthorized}
	>
		<p class="text-xs text-text-secondary">
			Drag and drop a session JSON file here
		</p>
		<p class="mt-1 text-xs text-text-muted">or</p>
		<label class="mt-2 cursor-pointer bg-bg-hover px-3 py-1 text-xs text-text-secondary transition-colors hover:text-text-primary">
			Browse files
			<input
				id="file-input"
				type="file"
				accept=".json"
				class="hidden"
				disabled={unauthorized}
				onchange={handleFileInput}
			/>
		</label>
	</div>

	<!-- Paste area -->
	<div class="mb-3">
		<label class="mb-1 block text-xs text-text-secondary" for="json-paste">
			Or paste raw JSON:
		</label>
		<textarea
			id="json-paste"
			bind:value={rawJson}
			oninput={handlePasteInput}
			placeholder={'{"version": "hail-1.0.0", ...}'}
			rows={6}
			class="w-full border border-border bg-bg-secondary p-3 font-mono text-xs text-text-primary placeholder-text-muted outline-none focus:border-accent"
			disabled={unauthorized}
		></textarea>
	</div>

	{#if parseError}
		<div class="mb-3 border border-error/30 bg-error/10 px-3 py-2 text-xs text-error">
			{parseError}
		</div>
	{/if}

	{#if parsedSession && tool}
		<div class="mb-3 border border-border bg-bg-secondary p-3">
			<h3 class="mb-2 text-sm font-medium text-text-primary">Preview</h3>
			<div class="flex items-center gap-3">
				<span
					class="tui-badge"
					class:tui-badge-tool={true}
				style="background-color: {tool.color}"
				>
					{tool.icon}
				</span>
				<div>
					<p class="text-sm font-medium text-text-primary">
						{parsedSession.context.title ?? 'Untitled Session'}
					</p>
					<p class="text-xs text-text-muted">
						{tool.label} &middot; {parsedSession.agent.model}
					</p>
				</div>
			</div>
			<div class="mt-2 flex gap-3 text-xs text-text-muted">
				<span>{parsedSession.stats.event_count} events</span>
				<span>{parsedSession.stats.message_count} messages</span>
				<span>{formatDuration(parsedSession.stats.duration_seconds)}</span>
			</div>
			{#if parsedSession.context.tags.length > 0}
				<div class="mt-2 flex flex-wrap gap-1">
					{#each parsedSession.context.tags as tag}
						<span class="text-xs text-text-secondary">#{tag}</span>
					{/each}
				</div>
			{/if}
		</div>

		{#if uploadError}
			<div class="mb-3 border border-error/30 bg-error/10 px-3 py-2 text-xs text-error">
				{uploadError}
			</div>
		{/if}

		<button
			onclick={handleUpload}
			disabled={unauthorized || uploading || !effectiveTeamId.trim()}
			class="w-full bg-accent px-4 py-2 text-xs font-medium text-white transition-colors hover:bg-accent/80 disabled:opacity-50"
		>
			{uploading ? 'Uploading...' : 'Upload Session'}
		</button>
	{/if}
</div>
