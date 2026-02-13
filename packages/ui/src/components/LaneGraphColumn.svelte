<script lang="ts">
import { DOT_OFFSET_PX, LANE_WIDTH_PX, RAIL_OFFSET_PX } from '../constants';

const {
	maxLane,
	activeLanes,
	lane,
	laneColor,
	isFork = false,
	forkLane,
	isMerge = false,
	mergeLane,
	getLaneColor,
}: {
	maxLane: number;
	activeLanes: number[];
	lane: number;
	laneColor: string;
	isFork?: boolean;
	forkLane?: number;
	isMerge?: boolean;
	mergeLane?: number;
	getLaneColor: (lane: number) => string;
} = $props();
</script>

<!-- Desktop graph column -->
<div class="lane-graph-col shrink-0 relative" style="width: {(maxLane + 1) * LANE_WIDTH_PX}px">
	<!-- Active lane rails -->
	{#each activeLanes as al}
		<div
			class="lane-rail"
			style="left: {al * LANE_WIDTH_PX + RAIL_OFFSET_PX}px; background: {getLaneColor(al)}"
		></div>
	{/each}
	<!-- Event dot -->
	<div
		class="lane-dot"
		style="left: {lane * LANE_WIDTH_PX + DOT_OFFSET_PX}px; background: {laneColor}"
	></div>
	<!-- Fork line (horizontal from main to new lane) -->
	{#if isFork && forkLane != null}
		<div
			class="lane-fork-line"
			style="left: {RAIL_OFFSET_PX}px; width: {forkLane * LANE_WIDTH_PX}px; background: {getLaneColor(forkLane)}"
		></div>
		<div
			class="lane-dot"
			style="left: {forkLane * LANE_WIDTH_PX + DOT_OFFSET_PX}px; background: {getLaneColor(forkLane)}"
		></div>
	{/if}
	<!-- Merge line (horizontal from sub-agent lane to main) -->
	{#if isMerge && mergeLane != null}
		<div
			class="lane-fork-line"
			style="left: {RAIL_OFFSET_PX}px; width: {mergeLane * LANE_WIDTH_PX}px; background: {getLaneColor(mergeLane)}"
		></div>
		<div
			class="lane-dot"
			style="left: {DOT_OFFSET_PX}px; background: {getLaneColor(0)}"
		></div>
	{/if}
</div>
<!-- Mobile lane indicator -->
{#if lane > 0}
	<div
		class="lane-mobile-bar"
		style="border-left-color: {laneColor}"
	></div>
{:else}
	<div class="lane-mobile-bar-none"></div>
{/if}

<style>
	.lane-graph-col {
		display: none;
	}
	.lane-mobile-bar {
		display: block;
		border-left: 3px solid transparent;
		padding-left: 8px;
		flex-shrink: 0;
	}
	.lane-mobile-bar-none {
		display: block;
		padding-left: 11px;
		flex-shrink: 0;
	}

	@media (min-width: 640px) {
		.lane-graph-col {
			display: block;
			min-height: 28px;
		}
		.lane-mobile-bar,
		.lane-mobile-bar-none {
			display: none;
		}
	}

	.lane-rail {
		position: absolute;
		top: 0;
		bottom: 0;
		width: 2px;
		opacity: 0.3;
	}

	.lane-dot {
		position: absolute;
		top: 10px;
		width: 8px;
		height: 8px;
		z-index: var(--z-graph-dot);
	}

	.lane-fork-line {
		position: absolute;
		top: 13px;
		height: 2px;
		z-index: var(--z-graph);
		opacity: 0.5;
	}
</style>
