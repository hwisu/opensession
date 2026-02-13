<script lang="ts">
const {
	toolName,
	payload,
}: {
	toolName: string;
	payload: Record<string, unknown> | null;
} = $props();

function statusColor(status: unknown): string {
	switch (String(status)) {
		case 'completed':
			return 'bg-success/20 text-success';
		case 'in_progress':
			return 'bg-accent/20 text-accent';
		case 'pending':
			return 'bg-warning/20 text-warning';
		case 'deleted':
			return 'bg-error/20 text-error';
		default:
			return 'bg-text-muted/10 text-text-muted';
	}
}
</script>

{#if toolName === 'TaskCreate' && payload}
	<span class="min-w-0 flex-1 truncate text-text-secondary">"{payload.subject ?? ''}"</span>
	<span class="shrink-0 rounded bg-yellow-500/20 px-1.5 py-0.5 text-[10px] text-yellow-400">pending</span>
{:else if toolName === 'TaskUpdate' && payload}
	<span class="shrink-0 font-mono text-text-secondary">#{payload.taskId ?? '?'}</span>
	{#if payload.status}
		<span class="shrink-0 rounded px-1.5 py-0.5 text-[10px] {statusColor(payload.status)}">{payload.status}</span>
	{/if}
	{#if payload.subject}
		<span class="min-w-0 flex-1 truncate text-text-muted">{payload.subject}</span>
	{/if}
{:else if toolName === 'SendMessage' && payload}
	<span class="shrink-0 text-text-secondary">&rarr; {payload.recipient ?? '?'}</span>
	{#if payload.summary}
		<span class="min-w-0 flex-1 truncate text-text-muted">{payload.summary}</span>
	{/if}
{:else if toolName === 'TeamCreate' && payload}
	<span class="shrink-0 font-mono text-text-secondary">{payload.team_name ?? ''}</span>
	{#if payload.description}
		<span class="min-w-0 flex-1 truncate text-text-muted">{payload.description}</span>
	{/if}
{:else if toolName === 'TeamDelete'}
	<span class="min-w-0 flex-1 truncate text-text-muted">delete team</span>
{:else if toolName === 'TaskGet' && payload}
	<span class="shrink-0 font-mono text-text-secondary">#{payload.taskId ?? '?'}</span>
{:else if toolName === 'TaskList'}
	<span class="min-w-0 flex-1 truncate text-text-muted">list all</span>
{/if}
