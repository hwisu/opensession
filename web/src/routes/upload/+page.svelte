<script lang="ts">
	import { uploadSession } from '$lib/api';
	import { getToolConfig, formatDuration } from '$lib/types';
	import type { Session } from '$lib/types';
	import { goto } from '$app/navigation';

	let parsedSession = $state<Session | null>(null);
	let parseError = $state<string | null>(null);
	let uploading = $state(false);
	let uploadError = $state<string | null>(null);
	let rawJson = $state('');
	let dragover = $state(false);
	let teamId = $state('');

	function parseJson(text: string) {
		parseError = null;
		parsedSession = null;
		try {
			const data = JSON.parse(text);
			if (!data.version || !data.session_id || !data.agent || !data.events) {
				throw new Error('Invalid session format: missing required fields (version, session_id, agent, events)');
			}
			parsedSession = data as Session;
		} catch (e) {
			parseError = e instanceof Error ? e.message : 'Invalid JSON';
		}
	}

	function handleFileInput(e: globalThis.Event) {
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
		dragover = true;
	}

	function handleDragLeave() {
		dragover = false;
	}

	function handlePasteInput() {
		if (rawJson.trim()) {
			parseJson(rawJson);
		}
	}

	async function handleUpload() {
		if (!parsedSession) return;
		uploading = true;
		uploadError = null;
		try {
			const result = await uploadSession(parsedSession, teamId);
			goto(`/session/${result.id}`);
		} catch (e) {
			uploadError = e instanceof Error ? e.message : 'Upload failed';
		} finally {
			uploading = false;
		}
	}

	let tool = $derived(parsedSession ? getToolConfig(parsedSession.agent.tool) : null);
</script>

<svelte:head>
	<title>Upload Session - opensession.io</title>
</svelte:head>

<div class="mx-auto max-w-2xl">
	<h1 class="mb-2 text-2xl font-bold text-white">Upload Session</h1>
	<p class="mb-6 text-sm text-text-secondary">
		Upload a HAIL session JSON file to share with the community
	</p>

	<!-- Drop zone -->
	<div
		role="button"
		tabindex="0"
		ondrop={handleDrop}
		ondragover={handleDragOver}
		ondragleave={handleDragLeave}
		onkeydown={(e) => e.key === 'Enter' && document.getElementById('file-input')?.click()}
		class="mb-4 flex cursor-pointer flex-col items-center justify-center rounded-lg border-2 border-dashed p-8 transition-colors {dragover
			? 'border-accent bg-accent/5'
			: 'border-border hover:border-border-light'}"
	>
		<p class="text-sm text-text-secondary">
			Drag and drop a session JSON file here
		</p>
		<p class="mt-1 text-xs text-text-muted">or</p>
		<label class="mt-2 cursor-pointer rounded bg-bg-hover px-4 py-1.5 text-sm text-text-secondary transition-colors hover:text-text-primary">
			Browse files
			<input
				id="file-input"
				type="file"
				accept=".json"
				class="hidden"
				onchange={handleFileInput}
			/>
		</label>
	</div>

	<!-- Paste area -->
	<div class="mb-4">
		<label class="mb-1 block text-sm text-text-secondary" for="json-paste">
			Or paste raw JSON:
		</label>
		<textarea
			id="json-paste"
			bind:value={rawJson}
			oninput={handlePasteInput}
			placeholder={'{"version": "hail-1.0.0", ...}'}
			rows={6}
			class="w-full rounded-lg border border-border bg-bg-secondary p-3 font-mono text-xs text-text-primary placeholder-text-muted outline-none transition-colors focus:border-accent"
		></textarea>
	</div>

	{#if parseError}
		<div class="mb-4 rounded-lg border border-error/30 bg-error/10 px-4 py-3 text-sm text-error">
			{parseError}
		</div>
	{/if}

	{#if parsedSession && tool}
		<div class="mb-4">
			<label class="mb-1 block text-sm text-text-secondary" for="team-id">
				Team ID:
			</label>
			<input
				id="team-id"
				type="text"
				bind:value={teamId}
				placeholder="Enter team ID"
				class="w-full rounded-lg border border-border bg-bg-secondary p-3 text-sm text-text-primary placeholder-text-muted outline-none transition-colors focus:border-accent"
			/>
		</div>

		<div class="mb-4 rounded-lg border border-border bg-bg-secondary p-4">
			<h3 class="mb-2 text-sm font-medium text-text-primary">Preview</h3>
			<div class="flex items-center gap-3">
				<div
					class="flex h-8 w-8 items-center justify-center rounded text-sm font-bold text-white"
					style="background-color: {tool.color}"
				>
					{tool.icon}
				</div>
				<div>
					<p class="text-sm font-medium text-text-primary">
						{parsedSession.context.title ?? 'Untitled Session'}
					</p>
					<p class="text-xs text-text-muted">
						{tool.label} &middot; {parsedSession.agent.model}
					</p>
				</div>
			</div>
			<div class="mt-3 flex gap-3 text-xs text-text-muted">
				<span>{parsedSession.stats.event_count} events</span>
				<span>{parsedSession.stats.message_count} messages</span>
				<span>{formatDuration(parsedSession.stats.duration_seconds)}</span>
			</div>
			{#if parsedSession.context.tags.length > 0}
				<div class="mt-2 flex flex-wrap gap-1">
					{#each parsedSession.context.tags as tag}
						<span class="rounded-full bg-bg-hover px-2 py-0.5 text-xs text-text-secondary">{tag}</span>
					{/each}
				</div>
			{/if}
		</div>

		{#if uploadError}
			<div class="mb-4 rounded-lg border border-error/30 bg-error/10 px-4 py-3 text-sm text-error">
				{uploadError}
			</div>
		{/if}

		<button
			onclick={handleUpload}
			disabled={uploading || !teamId.trim()}
			class="w-full rounded-lg bg-accent px-4 py-2.5 text-sm font-medium text-white transition-colors hover:bg-accent/80 disabled:opacity-50"
		>
			{uploading ? 'Uploading...' : 'Upload Session'}
		</button>
	{/if}
</div>
