<script lang="ts">
import TimelineView from './TimelineView.svelte';
import type { Session } from '../types';
import { parseHailInput } from '../hail-parse';
import {
	PARSER_CONFORMANCE_ROWS,
	conformanceCoverageScore,
	type ParserConformanceRow,
} from '../parser-conformance';

const {
	onNavigate = (_path: string) => {},
}: {
	onNavigate?: (path: string) => void;
} = $props();

const sampleRaw = `{"type":"header","version":"hail-1.0.0","session_id":"dx-sample-001","agent":{"provider":"anthropic","model":"claude-opus-4-6","tool":"claude-code","tool_version":"1.0.0"},"context":{"title":"DX Playground sample","description":"Parser playground baseline","tags":["dx","sample"],"created_at":"2026-02-16T10:00:00Z","updated_at":"2026-02-16T10:00:04Z","attributes":{}}}
{"type":"event","event_id":"e1","timestamp":"2026-02-16T10:00:01Z","event_type":{"type":"UserMessage"},"task_id":"t1","content":{"blocks":[{"type":"Text","text":"# Plan\\n\\n- Parse\\n- Render"}]},"duration_ms":null,"attributes":{}}
{"type":"event","event_id":"e2","timestamp":"2026-02-16T10:00:02Z","event_type":{"type":"ToolCall","data":{"name":"read_file"}},"task_id":"t1","content":{"blocks":[{"type":"Text","text":"read docs/parser-source-matrix.md"}]},"duration_ms":12,"attributes":{"semantic.call_id":"call_1"}}
{"type":"event","event_id":"e3","timestamp":"2026-02-16T10:00:03Z","event_type":{"type":"ToolResult","data":{"name":"read_file","is_error":false,"call_id":"call_1"}},"task_id":"t1","content":{"blocks":[{"type":"Code","language":"md","code":"# Parser Source Matrix\\n..."}]},"duration_ms":23,"attributes":{"semantic.call_id":"call_1"}}
{"type":"event","event_id":"e4","timestamp":"2026-02-16T10:00:04Z","event_type":{"type":"AgentMessage"},"task_id":"t1","content":{"blocks":[{"type":"Text","text":"Parsing baseline confirmed."}]},"duration_ms":null,"attributes":{}}
{"type":"stats","event_count":4,"message_count":2,"tool_call_count":1,"task_count":1,"duration_seconds":3,"total_input_tokens":42,"total_output_tokens":55}`;

let raw = $state(sampleRaw);
let parsed = $state<Session | null>(null);
let parseError = $state<string | null>(null);
const coverage = $derived(conformanceCoverageScore(PARSER_CONFORMANCE_ROWS));

const semanticStats = $derived.by(() => {
	if (!parsed) return [] as Array<{ type: string; count: number }>;
	const counts = new Map<string, number>();
	for (const event of parsed.events) {
		const type = event.event_type.type;
		counts.set(type, (counts.get(type) ?? 0) + 1);
	}
	return Array.from(counts.entries())
		.map(([type, count]) => ({ type, count }))
		.sort((a, b) => b.count - a.count || a.type.localeCompare(b.type));
});

function applyParse() {
	parseError = null;
	try {
		parsed = parseHailInput(raw);
	} catch (error) {
		parsed = null;
		parseError = error instanceof Error ? error.message : String(error);
	}
}

function loadSample() {
	raw = sampleRaw;
	applyParse();
}

function sourcePillClass(status: ParserConformanceRow['sourceStatus']): string {
	return status === 'open-source'
		? 'bg-success/15 text-success border-success/30'
		: 'bg-warning/15 text-warning border-warning/30';
}

$effect(() => {
	applyParse();
});
</script>

<svelte:head>
	<title>DX Lab - opensession.io</title>
</svelte:head>

<div class="mx-auto flex w-full max-w-7xl flex-col gap-4 pb-6">
	<section class="border border-border bg-bg-secondary/60 px-4 py-3">
		<p class="text-xs uppercase tracking-[0.22em] text-text-muted">DX Lab</p>
		<h1 class="mt-1 text-xl font-semibold text-text-primary sm:text-2xl">Parser Playground + Conformance</h1>
		<p class="mt-2 text-sm text-text-secondary">
			Use the same HAIL event model to inspect parse output, and track five-tool conformance from one view.
		</p>
		<div class="mt-3 flex flex-wrap gap-2 text-xs">
			<button
				type="button"
				class="border border-border px-2 py-1 text-text-secondary hover:border-accent hover:text-accent"
				onclick={() => onNavigate('/')}
			>
				Open Sessions
			</button>
			<button
				type="button"
				class="border border-border px-2 py-1 text-text-secondary hover:border-accent hover:text-accent"
				onclick={() => onNavigate('/docs')}
			>
				Open Docs
			</button>
		</div>
	</section>

	<section class="grid gap-4 lg:grid-cols-[minmax(20rem,36rem)_1fr]">
		<div class="border border-border bg-bg-secondary/40 p-3">
			<div class="mb-2 flex items-center justify-between gap-2">
				<h2 class="text-sm font-semibold text-text-primary">Parser Playground</h2>
				<div class="flex items-center gap-2">
					<button
						type="button"
						class="border border-border px-2 py-0.5 text-xs text-text-secondary hover:border-accent hover:text-accent"
						onclick={loadSample}
					>
						Load sample
					</button>
					<button
						type="button"
						class="border border-accent bg-accent/10 px-2 py-0.5 text-xs text-accent hover:bg-accent/15"
						onclick={applyParse}
					>
						Run parse
					</button>
				</div>
			</div>
			<label for="dx-raw-input" class="mb-1 block text-[11px] uppercase tracking-wider text-text-muted">
				HAIL JSON / JSONL
			</label>
			<textarea
				id="dx-raw-input"
				data-testid="dx-raw-input"
				bind:value={raw}
				class="h-[28rem] w-full resize-y border border-border bg-bg-primary p-2 font-mono text-xs text-text-primary outline-none focus:border-accent"
			></textarea>
			{#if parseError}
				<p data-testid="dx-parse-error" class="mt-2 border border-error/40 bg-error/10 px-2 py-1 text-xs text-error">
					{parseError}
				</p>
			{/if}
		</div>

		<div class="border border-border bg-bg-secondary/40 p-3">
			<h2 class="text-sm font-semibold text-text-primary">Semantic Output</h2>
			{#if parsed}
				<div class="mt-2 grid gap-2 text-xs text-text-secondary sm:grid-cols-2 lg:grid-cols-4">
					<div class="border border-border bg-bg-primary px-2 py-1">
						<div class="text-[10px] uppercase tracking-wider text-text-muted">Session</div>
						<div class="truncate text-text-primary">{parsed.session_id}</div>
					</div>
					<div class="border border-border bg-bg-primary px-2 py-1">
						<div class="text-[10px] uppercase tracking-wider text-text-muted">Tool</div>
						<div class="truncate text-text-primary">{parsed.agent.tool} / {parsed.agent.model}</div>
					</div>
					<div class="border border-border bg-bg-primary px-2 py-1">
						<div class="text-[10px] uppercase tracking-wider text-text-muted">Events</div>
						<div data-testid="dx-event-count" class="text-text-primary">{parsed.events.length}</div>
					</div>
					<div class="border border-border bg-bg-primary px-2 py-1">
						<div class="text-[10px] uppercase tracking-wider text-text-muted">Tasks</div>
						<div class="text-text-primary">{parsed.stats.task_count}</div>
					</div>
				</div>

				<div class="mt-3 border border-border bg-bg-primary p-2">
					<p class="mb-1 text-[11px] uppercase tracking-wider text-text-muted">Event Type Distribution</p>
					<div class="flex flex-wrap gap-1 text-xs">
						{#each semanticStats as stat}
							<span class="border border-border bg-bg-secondary px-2 py-0.5 text-text-secondary">
								{stat.type}: {stat.count}
							</span>
						{/each}
					</div>
				</div>

				<div class="mt-3 border border-border bg-bg-primary p-2">
					<p class="mb-2 text-[11px] uppercase tracking-wider text-text-muted">Human Readable Preview</p>
					<TimelineView events={parsed.events} />
				</div>
			{:else}
				<p class="mt-2 text-xs text-text-muted">Parse a session to view semantic output.</p>
			{/if}
		</div>
	</section>

	<section class="border border-border bg-bg-secondary/40 p-3">
		<div class="mb-3 flex items-start justify-between gap-3">
			<div>
				<h2 class="text-sm font-semibold text-text-primary">Parser Conformance Dashboard</h2>
				<p class="mt-1 text-xs text-text-secondary">
					Baseline fixtures and company-source alignment for Codex / Claude / Cursor / Gemini / OpenCode.
				</p>
			</div>
			<div class="border border-border bg-bg-primary px-2 py-1 text-right">
				<div class="text-[10px] uppercase tracking-wider text-text-muted">Coverage score</div>
				<div data-testid="dx-coverage-score" class="text-lg font-semibold text-accent">{coverage}%</div>
			</div>
		</div>

		<div class="overflow-x-auto">
			<table data-testid="dx-conformance-table" class="min-w-full border-collapse text-xs">
				<thead>
					<tr class="border-b border-border text-left text-text-muted">
						<th class="px-2 py-2 font-medium">Tool</th>
						<th class="px-2 py-2 font-medium">Source</th>
						<th class="px-2 py-2 font-medium">Fixtures</th>
						<th class="px-2 py-2 font-medium">Critical checks</th>
						<th class="px-2 py-2 font-medium">Refs</th>
						<th class="px-2 py-2 font-medium">Verified</th>
					</tr>
				</thead>
				<tbody>
					{#each PARSER_CONFORMANCE_ROWS as row}
						<tr class="border-b border-border/60 align-top">
							<td class="px-2 py-2 text-text-primary">{row.tool}</td>
							<td class="px-2 py-2">
								<span class={`inline-block border px-1.5 py-0.5 ${sourcePillClass(row.sourceStatus)}`}>
									{row.sourceStatus}
								</span>
							</td>
							<td class="px-2 py-2 text-text-secondary">{row.fixtureCount}</td>
							<td class="px-2 py-2">
								<ul class="space-y-1 text-text-secondary">
									{#each row.criticalChecks as check}
										<li>{check}</li>
									{/each}
								</ul>
							</td>
							<td class="px-2 py-2">
								<div class="flex flex-col gap-1">
									{#each row.references as ref}
										<a class="text-accent hover:underline" href={ref.url} target="_blank" rel="noreferrer">
											{ref.label}
										</a>
									{/each}
								</div>
							</td>
							<td class="px-2 py-2 text-text-secondary">{row.lastVerifiedAt}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	</section>
</div>
