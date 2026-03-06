<script lang="ts">
import { goto } from '$app/navigation';
import { page } from '$app/stores';
import { untrack } from 'svelte';
import {
	buildNativeFilterOptions,
	buildUnifiedFilterOptions,
	createSourcePreviewModel,
	createSourcePreviewModelState,
	getApiCapabilities,
	getParsePreviewError,
	previewSessionFromGitSource,
	previewSessionFromGithubSource,
	type Session,
	type SessionViewMode,
} from '@opensession/ui';
import { ParseSourceBanner, ParserSelectPanel, SessionRenderPage } from '@opensession/ui/components';

const state = $state(createSourcePreviewModelState());

const model = createSourcePreviewModel(state, {
	getApiCapabilities,
	previewSessionFromGithubSource,
	previewSessionFromGitSource,
	getParsePreviewError,
	buildUnifiedFilterKeys: (session) =>
		buildUnifiedFilterOptions(session.events).map((option) => option.key),
	buildNativeFilterKeys: (session) =>
		buildNativeFilterOptions(session.events).map((option) => option.key),
	replaceStateUrl: (url) => goto(url, { replaceState: true, keepFocus: true, noScroll: true }),
});

function toggleUnifiedFilter(key: string) {
	model.toggleUnifiedFilter(key);
}

function toggleNativeFilter(key: string) {
	model.toggleNativeFilter(key);
}

function changeViewMode(mode: SessionViewMode) {
	model.changeViewMode(mode);
}

function selectParser(parserId: string) {
	model.selectParser(parserId);
}

$effect(() => {
	const href = $page.url.href;
	untrack(() => {
		void model.loadFromLocation({
			provider: $page.params.provider,
			segments: $page.params.segments,
			pathname: $page.url.pathname,
			search: $page.url.search,
			href,
		});
	});
});
</script>

{#if state.pageState === 'loading' || state.pageState === 'idle'}
	<div class="py-16 text-center text-xs text-text-muted">Loading source preview...</div>
{:else if state.pageState === 'unsupported'}
	<div class="mx-auto max-w-3xl border border-border bg-bg-secondary p-6 text-sm text-text-secondary">
		This deployment does not support source parse preview.
	</div>
{:else if state.pageState === 'select_parser'}
	<div class="mx-auto max-w-3xl space-y-3">
		{#if state.errorMessage}
			<div class="border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">
				{state.errorMessage}
			</div>
		{/if}
		<ParserSelectPanel
			candidates={state.parserCandidates}
			parserHint={state.parserHint}
			loading={false}
			onSelect={selectParser}
		/>
	</div>
{:else if state.pageState === 'error'}
	<div class="mx-auto max-w-3xl border border-error/30 bg-error/10 px-4 py-3 text-sm text-error">
		{state.errorMessage ?? 'Failed to load source.'}
	</div>
{:else if state.preview && state.currentRoute}
	<div class="space-y-3">
		<ParseSourceBanner
			source={state.preview.source}
			parserUsed={state.preview.parser_used}
			warnings={state.preview.warnings}
		/>
		<SessionRenderPage
			session={state.preview.session as Session}
			viewMode={state.viewMode}
			unifiedFilters={state.unifiedFilters}
			nativeFilters={state.nativeFilters}
			onViewModeChange={changeViewMode}
			onToggleUnifiedFilter={toggleUnifiedFilter}
			onToggleNativeFilter={toggleNativeFilter}
		/>
	</div>
{:else}
	<div class="py-16 text-center text-xs text-text-muted">No preview data.</div>
{/if}
