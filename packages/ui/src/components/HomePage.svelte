<script lang="ts">
import { listTeams } from '../api';
import SessionListPage from './SessionListPage.svelte';
import TeamsListPage from './TeamsListPage.svelte';

const { onNavigate }: { onNavigate: (path: string) => void } = $props();

type Tab = 'sessions' | 'teams';
let activeTab = $state<Tab>('sessions');
let tabResolved = $state(false);

const tabs: { value: Tab; label: string }[] = [
	{ value: 'sessions', label: 'Sessions' },
	{ value: 'teams', label: 'Teams' },
];

$effect(() => {
	listTeams()
		.then((res) => {
			if (res.teams.length > 0) activeTab = 'teams';
		})
		.catch(() => {})
		.finally(() => {
			tabResolved = true;
		});
});
</script>

{#if !tabResolved}
	<div class="py-8 text-center text-xs text-text-muted">Loading...</div>
{:else}
	<div class="flex h-full flex-col">
		<div class="flex shrink-0 items-center gap-1 border-b border-border px-2 py-1.5" role="tablist" aria-label="Home tabs">
			{#each tabs as tab}
				<button
					role="tab"
					aria-selected={activeTab === tab.value}
					onclick={() => { activeTab = tab.value; }}
					class="px-2 py-0.5 text-xs transition-colors
						{activeTab === tab.value
						? 'bg-accent text-white'
						: 'text-text-secondary hover:text-text-primary'}"
				>
					{activeTab === tab.value ? `[${tab.label}]` : tab.label}
				</button>
			{/each}
		</div>

		<div class="min-h-0 flex-1">
			{#if activeTab === 'sessions'}
				<SessionListPage {onNavigate} />
			{:else}
				<TeamsListPage />
			{/if}
		</div>
	</div>
{/if}
