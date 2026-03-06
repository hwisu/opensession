<script lang="ts">
import type { SettingsSectionNavItem } from './models';

const {
	items = [],
	activeId,
	onSelect,
}: {
	items?: SettingsSectionNavItem[];
	activeId: string;
	onSelect: (sectionId: string) => void;
} = $props();

function settingsNavButtonClasses(active: boolean): string {
	return active
		? 'border-accent/40 bg-accent/5 text-text-primary shadow-[0_8px_20px_rgba(15,23,42,0.08)]'
		: 'border-border/70 bg-bg-primary text-text-secondary';
}
</script>

<aside
	class="xl:sticky xl:top-4"
	data-testid="settings-left-tabs"
>
	<div class="overflow-x-auto border border-border bg-bg-secondary p-3 shadow-[0_14px_40px_rgba(15,23,42,0.08)]">
		<p class="text-[11px] font-semibold uppercase tracking-[0.08em] text-text-muted">
			Settings Tabs
		</p>
		<div class="mt-3 flex gap-2 xl:flex-col">
			{#each items as item}
				<button
					type="button"
					data-testid={`settings-nav-${item.id}`}
					data-state={activeId === item.id ? 'active' : 'idle'}
					aria-current={activeId === item.id ? 'page' : undefined}
					onclick={() => onSelect(item.id)}
					class={`min-w-[11rem] border px-3 py-2 text-left transition-colors xl:min-w-0 ${settingsNavButtonClasses(activeId === item.id)}`}
				>
					<p class="text-xs font-semibold">{item.label}</p>
					<p class="mt-1 text-[11px] text-text-secondary">{item.detail}</p>
				</button>
			{/each}
		</div>
	</div>
</aside>
