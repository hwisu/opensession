<script lang="ts">
import { appLocale } from '../i18n';
import type { ParseCandidate } from '../types';

const {
	candidates,
	parserHint = null,
	loading = false,
	onSelect = (_parserId: string) => {},
}: {
	candidates: ParseCandidate[];
	parserHint?: string | null;
	loading?: boolean;
	onSelect?: (parserId: string) => void;
} = $props();

const isKorean = $derived($appLocale === 'ko');

function localize(en: string, ko: string): string {
	return isKorean ? ko : en;
}
</script>

<section class="border border-border bg-bg-secondary p-3">
	<h2 class="text-sm font-semibold text-text-primary">
		{localize('Parser selection required', '파서 선택이 필요합니다')}
	</h2>
	<p class="mt-1 text-xs text-text-secondary">
		{localize(
			'Auto-detection could not produce a stable parse. Select a parser and retry.',
			'자동 감지로 안정적인 파싱 결과를 만들지 못했습니다. 파서를 선택한 뒤 다시 시도하세요.',
		)}
	</p>

	<div class="mt-3 grid gap-2">
		{#each candidates as candidate}
			<button
				onclick={() => onSelect(candidate.id)}
				disabled={loading}
				class="flex items-center justify-between border border-border bg-bg px-3 py-2 text-left text-xs transition-colors hover:border-accent disabled:opacity-50"
			>
				<div class="min-w-0">
					<div class="font-medium text-text-primary">
						{candidate.id}
						{#if parserHint === candidate.id}
							<span class="ml-1 text-accent">{localize('(selected)', '(선택됨)')}</span>
						{/if}
					</div>
					<div class="truncate text-text-muted">{candidate.reason}</div>
				</div>
				<div class="text-text-secondary">{candidate.confidence}%</div>
			</button>
		{/each}
	</div>
</section>
