<script lang="ts">
import { onDestroy } from 'svelte';
import { uploadSession } from '../api';
import type { Session } from '../types';
import { formatDuration, getToolConfig } from '../types';

const {
	onSuccess,
}: {
	onSuccess: (id: string) => void;
} = $props();

const PASTE_PARSE_DEBOUNCE_MS = 180;

let parsedSession = $state<Session | null>(null);
let parseError = $state<string | null>(null);
let uploading = $state(false);
let uploadError = $state<string | null>(null);
let rawJson = $state('');
let dragover = $state(false);
let dragDepth = $state(0);
let parseTimer: ReturnType<typeof setTimeout> | null = null;

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
	const input = e.target as HTMLInputElement;
	const file = input.files?.[0];
	if (!file) return;
	const reader = new FileReader();
	reader.onload = () => {
		rawJson = reader.result as string;
		if (parseTimer !== null) {
			clearTimeout(parseTimer);
			parseTimer = null;
		}
		parseJson(rawJson);
	};
	reader.readAsText(file);
}

function handleDrop(e: DragEvent) {
	e.preventDefault();
	if (dragover) dragover = false;
	dragDepth = 0;
	const file = e.dataTransfer?.files[0];
	if (!file) return;
	const reader = new FileReader();
	reader.onload = () => {
		rawJson = reader.result as string;
		if (parseTimer !== null) {
			clearTimeout(parseTimer);
			parseTimer = null;
		}
		parseJson(rawJson);
	};
	reader.readAsText(file);
}

function handleDragEnter(e: DragEvent) {
	e.preventDefault();
	dragDepth += 1;
	if (!dragover) dragover = true;
}

function handleDragOver(e: DragEvent) {
	e.preventDefault();
	if (!dragover) dragover = true;
}

function handleDragLeave(e: DragEvent) {
	e.preventDefault();
	dragDepth = Math.max(0, dragDepth - 1);
	if (dragDepth === 0 && dragover) {
		dragover = false;
	}
}

function scheduleParseFromTextarea() {
	if (parseTimer !== null) {
		clearTimeout(parseTimer);
	}
	parseTimer = setTimeout(() => {
		parseTimer = null;
		parseJson(rawJson);
	}, PASTE_PARSE_DEBOUNCE_MS);
}

function handlePasteInput() {
	if (rawJson.trim()) {
		scheduleParseFromTextarea();
	}
}

async function handleUpload() {
	if (!parsedSession) return;
	uploading = true;
	uploadError = null;
	try {
		const result = await uploadSession(parsedSession);
		onSuccess(result.id);
	} catch (e) {
		uploadError = e instanceof Error ? e.message : 'Upload failed';
	} finally {
		uploading = false;
	}
}

const tool = $derived(parsedSession ? getToolConfig(parsedSession.agent.tool) : null);

onDestroy(() => {
	if (parseTimer !== null) {
		clearTimeout(parseTimer);
		parseTimer = null;
	}
});
</script>

<svelte:head>
	<title>Upload Session - opensession.io</title>
</svelte:head>

<div class="mx-auto max-w-2xl">
	<h1 class="mb-2 text-lg font-bold text-text-primary">Upload Session</h1>
	<p class="mb-4 text-sm text-text-secondary">
		Upload a HAIL session JSON file into your local/public session stream.
	</p>

	<!-- Drop zone -->
	<div
		role="button"
		tabindex="0"
		ondrop={handleDrop}
		ondragenter={handleDragEnter}
		ondragover={handleDragOver}
		ondragleave={handleDragLeave}
		onkeydown={(e) => e.key === 'Enter' && document.getElementById('file-input')?.click()}
		class="mb-3 flex cursor-pointer flex-col items-center justify-center border-2 border-dashed p-6 transition-colors {dragover
			? 'border-accent bg-accent/5'
			: 'border-border hover:border-border-light'}"
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
			disabled={uploading}
			class="w-full bg-accent px-4 py-2 text-xs font-medium text-white transition-colors hover:bg-accent/80 disabled:opacity-50"
		>
			{uploading ? 'Uploading...' : 'Upload Session'}
		</button>
	{/if}
</div>
