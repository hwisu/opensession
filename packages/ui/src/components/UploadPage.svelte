<script lang="ts">
import { onDestroy } from 'svelte';
import { getParsePreviewError, previewSessionFromInlineSource, uploadSession } from '../api';
import { parseHailInput } from '../hail-parse';
import type { ParsePreviewResponse, Session } from '../types';
import { formatDuration, getToolConfig } from '../types';
import ParserSelectPanel from './ParserSelectPanel.svelte';

const {
	onSuccess,
	ingestPreviewEnabled = true,
}: {
	onSuccess: (id: string) => void;
	ingestPreviewEnabled?: boolean;
} = $props();

const PASTE_PARSE_DEBOUNCE_MS = 280;

let parsedSession = $state<Session | null>(null);
let parsePreview = $state<ParsePreviewResponse | null>(null);
let parseError = $state<string | null>(null);
let uploading = $state(false);
let uploadError = $state<string | null>(null);
let rawInput = $state('');
let sourceFilename = $state('session.jsonl');
let parsing = $state(false);
let parserHint = $state<string | null>(null);
let parserSelectionCandidates = $state<ParsePreviewResponse['parser_candidates']>([]);
let dragover = $state(false);
let dragDepth = $state(0);
let parseTimer: ReturnType<typeof setTimeout> | null = null;

const tool = $derived(parsedSession ? getToolConfig(parsedSession.agent.tool) : null);

function encodeUtf8Base64(input: string): string {
	const bytes = new TextEncoder().encode(input);
	let binary = '';
	for (const byte of bytes) {
		binary += String.fromCharCode(byte);
	}
	return btoa(binary);
}

function clearPreviewState() {
	parsedSession = null;
	parsePreview = null;
	parserSelectionCandidates = [];
}

async function parseInputWithPreview(filename: string, raw: string) {
	if (!raw.trim()) {
		clearPreviewState();
		parseError = null;
		return;
	}

	parsing = true;
	parseError = null;
	uploadError = null;
	sourceFilename = filename.trim() || 'session.jsonl';

	if (!ingestPreviewEnabled) {
		try {
			parsedSession = parseHailInput(raw);
			parsePreview = null;
			parserSelectionCandidates = [];
			parseError = null;
		} catch (error) {
			clearPreviewState();
			parserSelectionCandidates = [];
			const message = error instanceof Error ? error.message : 'Parse failed';
			parseError = `Ingest preview is disabled in this deployment. Provide a valid HAIL JSON/JSONL payload. ${message}`;
		} finally {
			parsing = false;
		}
		return;
	}

	try {
		const preview = await previewSessionFromInlineSource({
			filename: sourceFilename,
			content_base64: encodeUtf8Base64(raw),
			parser_hint: parserHint ?? undefined,
		});
		parsePreview = preview;
		parsedSession = preview.session as Session;
		parserSelectionCandidates = [];
	} catch (error) {
		clearPreviewState();
		const parseApiError = getParsePreviewError(error);
		if (parseApiError?.code === 'parser_selection_required') {
			parserSelectionCandidates = parseApiError.parser_candidates ?? [];
		} else {
			parserSelectionCandidates = [];
		}
		parseError = parseApiError?.message ?? (error instanceof Error ? error.message : 'Parse failed');
	} finally {
		parsing = false;
	}
}

function parseNow() {
	void parseInputWithPreview(sourceFilename, rawInput);
}

function scheduleParseFromTextarea() {
	if (parseTimer !== null) {
		clearTimeout(parseTimer);
	}
	parseTimer = setTimeout(() => {
		parseTimer = null;
		parseNow();
	}, PASTE_PARSE_DEBOUNCE_MS);
}

async function handleParserSelect(parserId: string) {
	parserHint = parserId;
	await parseInputWithPreview(sourceFilename, rawInput);
}

function handleFileInput(e: globalThis.Event) {
	const input = e.target as HTMLInputElement;
	const file = input.files?.[0];
	if (!file) return;

	sourceFilename = file.name;
	const reader = new FileReader();
	reader.onload = () => {
		rawInput = String(reader.result ?? '');
		if (parseTimer !== null) {
			clearTimeout(parseTimer);
			parseTimer = null;
		}
		parseNow();
	};
	reader.readAsText(file);
}

function handleDrop(e: DragEvent) {
	e.preventDefault();
	if (dragover) dragover = false;
	dragDepth = 0;
	const file = e.dataTransfer?.files[0];
	if (!file) return;

	sourceFilename = file.name;
	const reader = new FileReader();
	reader.onload = () => {
		rawInput = String(reader.result ?? '');
		if (parseTimer !== null) {
			clearTimeout(parseTimer);
			parseTimer = null;
		}
		parseNow();
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

function handlePasteInput() {
	if (!rawInput.trim()) {
		clearPreviewState();
		parseError = null;
		return;
	}
	scheduleParseFromTextarea();
}

async function handleUpload() {
	if (!parsedSession) return;
	uploading = true;
	uploadError = null;
	try {
		const result = await uploadSession(parsedSession);
		onSuccess(result.id);
	} catch (error) {
		uploadError = error instanceof Error ? error.message : 'Upload failed';
	} finally {
		uploading = false;
	}
}

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
	{#if ingestPreviewEnabled}
		<p class="mb-4 text-sm text-text-secondary">
			Upload a raw session file. The server will auto-detect parser and normalize it to HAIL.
		</p>
	{:else}
		<p class="mb-4 text-sm text-text-secondary">
			This deployment accepts pre-parsed HAIL JSON/JSONL only. Auto-detect parsing is disabled.
		</p>
	{/if}

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
			Drag and drop a session file here
		</p>
		<p class="mt-1 text-xs text-text-muted">or</p>
		<label class="mt-2 cursor-pointer bg-bg-hover px-3 py-1 text-xs text-text-secondary transition-colors hover:text-text-primary">
			Browse files
			<input
				id="file-input"
				type="file"
				class="hidden"
				onchange={handleFileInput}
			/>
		</label>
	</div>

	<div class="mb-3">
		<label class="mb-1 block text-xs text-text-secondary" for="source-filename">
			Filename
		</label>
		<input
			id="source-filename"
			type="text"
			bind:value={sourceFilename}
			oninput={scheduleParseFromTextarea}
			class="w-full border border-border bg-bg-secondary px-3 py-2 font-mono text-xs text-text-primary outline-none focus:border-accent"
			placeholder="session.jsonl"
		/>
	</div>

	<div class="mb-3">
		<label class="mb-1 block text-xs text-text-secondary" for="raw-input">
			Raw session content
		</label>
		<textarea
			id="raw-input"
			bind:value={rawInput}
			oninput={handlePasteInput}
			rows={8}
			placeholder="Paste JSON/JSONL/session export here..."
			class="w-full border border-border bg-bg-secondary p-3 font-mono text-xs text-text-primary placeholder-text-muted outline-none focus:border-accent"
		></textarea>
	</div>

	{#if parsing}
		<div class="mb-3 border border-border bg-bg-secondary px-3 py-2 text-xs text-text-muted">
			Parsing source preview...
		</div>
	{/if}

	{#if parseError}
		<div class="mb-3 border border-error/30 bg-error/10 px-3 py-2 text-xs text-error">
			{parseError}
		</div>
	{/if}

	{#if parserSelectionCandidates.length > 0}
		<div class="mb-3">
			<ParserSelectPanel
				candidates={parserSelectionCandidates}
				{parserHint}
				loading={parsing}
				onSelect={handleParserSelect}
			/>
		</div>
	{/if}

	{#if parsedSession && tool}
		<div class="mb-3 border border-border bg-bg-secondary p-3">
			<h3 class="mb-2 text-sm font-medium text-text-primary">Preview</h3>
			<div class="flex items-center gap-3">
				<span
					class="tui-badge tui-badge-tool"
					style="background-color: {tool.color}"
				>
					{tool.icon}
				</span>
				<div class="min-w-0">
					<p class="truncate text-sm font-medium text-text-primary">
						{parsedSession.context.title ?? 'Untitled Session'}
					</p>
					<p class="truncate text-xs text-text-muted">
						{tool.label} &middot; {parsedSession.agent.model}
						{#if parsePreview}
							&middot; parser {parsePreview.parser_used}
						{/if}
					</p>
				</div>
			</div>
			<div class="mt-2 flex gap-3 text-xs text-text-muted">
				<span>{parsedSession.stats.event_count} events</span>
				<span>{parsedSession.stats.message_count} messages</span>
				<span>{formatDuration(parsedSession.stats.duration_seconds)}</span>
			</div>
			{#if parsePreview?.warnings && parsePreview.warnings.length > 0}
				<ul class="mt-2 space-y-1 text-xs text-warning">
					{#each parsePreview.warnings as warning}
						<li>{warning}</li>
					{/each}
				</ul>
			{/if}
		</div>

		{#if uploadError}
			<div class="mb-3 border border-error/30 bg-error/10 px-3 py-2 text-xs text-error">
				{uploadError}
			</div>
		{/if}

		<button
			onclick={handleUpload}
			disabled={uploading || parsing}
			class="w-full bg-accent px-4 py-2 text-xs font-medium text-white transition-colors hover:bg-accent/80 disabled:opacity-50"
		>
			{uploading ? 'Uploading...' : 'Upload Session'}
		</button>
	{/if}
</div>
