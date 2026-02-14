import { FILEREAD_LOOKAHEAD } from './constants';
import type {
	CollapsedTaskItem,
	ConsecutiveGroupItem,
	DisplayItem,
	LaneEvent,
	PairedToolCallItem,
	TaskInfo,
} from './timeline-types';
import type { Event } from './types';

// --- Lane colors ---

const LANE_COLORS = [
	'var(--color-text-primary)',
	'var(--color-accent)',
	'#f97316',
	'#a855f7',
	'#06b6d4',
	'#ec4899',
	'#14b8a6',
	'#f59e0b',
];

export function getLaneColor(lane: number): string {
	return LANE_COLORS[lane % LANE_COLORS.length];
}

// --- Pure lane computations ---

export function computeLaneEvents(events: Event[]): LaneEvent[] {
	const taskLanes = new Map<string, number>();
	const activeLanes = new Set<number>([0]);
	const freeLanes: number[] = [];
	let nextLane = 1;
	const result: LaneEvent[] = [];

	for (const event of events) {
		const eventTypeName = event.event_type.type;
		const taskId = event.task_id;

		let lane = 0;
		let isFork = false;
		let isMerge = false;
		let forkLane: number | undefined;
		let mergeLane: number | undefined;

		if (eventTypeName === 'TaskStart' && taskId) {
			const newLane = freeLanes.shift() ?? nextLane++;
			taskLanes.set(taskId, newLane);
			activeLanes.add(newLane);
			lane = 0;
			isFork = true;
			forkLane = newLane;
		} else if (eventTypeName === 'TaskEnd' && taskId) {
			lane = taskLanes.get(taskId) ?? 0;
			isMerge = true;
			mergeLane = lane;
			activeLanes.delete(lane);
			freeLanes.push(lane);
			taskLanes.delete(taskId);
		} else if (taskId) {
			lane = taskLanes.get(taskId) ?? 0;
		}

		result.push({
			event,
			lane,
			activeLanes: [...activeLanes].sort((a, b) => a - b),
			isFork,
			isMerge,
			forkLane,
			mergeLane,
			laneColor: getLaneColor(lane),
		});
	}

	return result;
}

export function computeMaxLane(laneEvents: LaneEvent[]): number {
	let max = 0;
	for (const laneEvent of laneEvents) {
		for (const l of laneEvent.activeLanes) {
			if (l > max) max = l;
		}
		if (laneEvent.forkLane != null && laneEvent.forkLane > max) max = laneEvent.forkLane;
	}
	return max;
}

export function computeTaskInfoMap(laneEvents: LaneEvent[]): Map<string, TaskInfo> {
	const map = new Map<string, TaskInfo>();
	const startTimes = new Map<string, number>();

	function extractTaskPurpose(event: Event): string {
		if ('data' in event.event_type) {
			const data = event.event_type.data as { title?: string };
			if (data.title?.trim()) {
				return data.title.trim();
			}
		}

		for (const block of event.content.blocks) {
			if (block.type === 'Text' && block.text.trim()) return block.text.trim();
		}

		return 'Sub-agent';
	}

	for (const laneEvent of laneEvents) {
		const eventTypeName = laneEvent.event.event_type.type;
		const taskId = laneEvent.event.task_id;
		if (!taskId) continue;

		if (eventTypeName === 'TaskStart') {
			const startedAt = laneEvent.event.timestamp;
			const purpose = extractTaskPurpose(laneEvent.event);
			map.set(taskId, {
				taskId,
				title: purpose,
				purpose,
				eventCount: 0,
				durationMs: 0,
				startedAt,
				endedAt: '',
				lane: laneEvent.forkLane ?? 0,
				activeLanesAtStart: [...laneEvent.activeLanes],
				activeLanesAtEnd: [],
			});
			startTimes.set(taskId, new Date(laneEvent.event.timestamp).getTime());
		} else if (eventTypeName === 'TaskEnd') {
			const info = map.get(taskId);
			if (info) {
				const startTime = startTimes.get(taskId);
				info.endedAt = laneEvent.event.timestamp;
				if (startTime) {
					info.durationMs = new Date(laneEvent.event.timestamp).getTime() - startTime;
				}
				info.activeLanesAtEnd = [...laneEvent.activeLanes];
			}
		} else {
			const info = map.get(taskId);
			if (info) info.eventCount++;
		}
	}

	return map;
}

// --- Consecutive group helpers ---

export function consecutiveGroupKey(event: Event): string | null {
	const et = event.event_type;
	switch (et.type) {
		case 'FileRead':
			return 'FileRead';
		case 'CodeSearch':
			return 'CodeSearch';
		case 'FileSearch':
			return 'FileSearch';
		case 'WebSearch':
			return 'WebSearch';
		case 'WebFetch':
			return 'WebFetch';
		case 'ToolCall':
			return `ToolCall:${et.data.name}`;
		case 'ToolResult':
			return `ToolResult:${et.data.name}`;
		default:
			return null;
	}
}

export function consecutiveGroupDisplayName(groupKey: string): string {
	if (groupKey.startsWith('ToolCall:')) return groupKey.slice(9);
	if (groupKey.startsWith('ToolResult:')) return `${groupKey.slice(11)} result`;
	return groupKey.replace(/([A-Z])/g, ' $1').trim();
}

export function consecutiveGroupSummary(groupEvents: LaneEvent[]): string {
	const names = groupEvents
		.map((e) => {
			const et = e.event.event_type;
			switch (et.type) {
				case 'FileRead':
				case 'FileEdit':
				case 'FileCreate':
				case 'FileDelete': {
					const parts = et.data.path.split('/');
					return parts[parts.length - 1];
				}
				case 'CodeSearch':
					return et.data.query;
				case 'FileSearch':
					return et.data.pattern;
				case 'WebSearch':
					return et.data.query;
				case 'WebFetch': {
					try {
						return new URL(et.data.url).hostname;
					} catch {
						return et.data.url;
					}
				}
				case 'ShellCommand': {
					const cmd = et.data.command;
					return cmd.length > 30 ? `${cmd.slice(0, 27)}...` : cmd;
				}
				default:
					return '';
			}
		})
		.filter(Boolean);

	if (names.length === 0) return '';
	if (names.length <= 3) return names.join(', ');
	return `${names.slice(0, 2).join(', ')}, +${names.length - 2} more`;
}

// --- Task breakdown & formatting ---

export function taskBreakdown(laneEvents: LaneEvent[], taskId: string): string {
	let edits = 0,
		reads = 0,
		shells = 0,
		tools = 0,
		messages = 0;
	for (const laneEvent of laneEvents) {
		if (laneEvent.event.task_id !== taskId) continue;
		const t = laneEvent.event.event_type.type;
		if (t === 'FileEdit' || t === 'FileCreate') edits++;
		else if (t === 'FileRead') reads++;
		else if (t === 'ShellCommand') shells++;
		else if (t === 'ToolCall') tools++;
		else if (t === 'AgentMessage') messages++;
	}
	const parts: string[] = [];
	if (edits > 0) parts.push(`${edits} edit${edits > 1 ? 's' : ''}`);
	if (reads > 0) parts.push(`${reads} read${reads > 1 ? 's' : ''}`);
	if (shells > 0) parts.push(`${shells} shell`);
	if (tools > 0) parts.push(`${tools} tool${tools > 1 ? 's' : ''}`);
	if (messages > 0) parts.push(`${messages} msg${messages > 1 ? 's' : ''}`);
	return parts.join(', ');
}

export function formatMs(ms: number): string {
	if (ms < 1000) return `${ms}ms`;
	const s = Math.round(ms / 1000);
	if (s < 60) return `${s}s`;
	const m = Math.floor(s / 60);
	return `${m}m ${s % 60}s`;
}

// --- Display pipeline ---

/** Step 1: Apply task view mode + filters, producing lane events or collapsed summaries. */
export function applyTaskViewMode(
	laneEvents: LaneEvent[],
	taskViewMode: 'chronological' | 'summary-start',
	collapsedTasks: Set<string>,
	taskInfoMap: Map<string, TaskInfo>,
	matchesFilter: (evType: string) => boolean,
): (LaneEvent | CollapsedTaskItem)[] {
	const result: (LaneEvent | CollapsedTaskItem)[] = [];
	const skippingTasks = new Set<string>();

	for (const le of laneEvents) {
		const evType = le.event.event_type.type;
		const taskId = le.event.task_id;

		if (evType === 'TaskStart' && taskId) {
			if (taskViewMode === 'summary-start') {
				skippingTasks.add(taskId);
				const info = taskInfoMap.get(taskId);
				if (info) {
					result.push({
						kind: 'collapsed',
						taskId,
						info,
						activeLanes: le.activeLanes,
						lane: le.forkLane ?? 0,
					});
				}
				continue;
			} else {
				// Chronological: manual toggle
				if (collapsedTasks.has(taskId)) {
					skippingTasks.add(taskId);
					const info = taskInfoMap.get(taskId);
					if (info) {
						result.push({
							kind: 'collapsed',
							taskId,
							info,
							activeLanes: le.activeLanes,
							lane: le.forkLane ?? 0,
						});
					}
					continue;
				}
				result.push(le);
				continue;
			}
		}

		if (evType === 'TaskEnd' && taskId) {
			if (skippingTasks.has(taskId)) {
				skippingTasks.delete(taskId);
				continue;
			}
			result.push(le);
			continue;
		}

		if (taskId && skippingTasks.has(taskId)) continue;
		if (!matchesFilter(evType)) continue;
		result.push(le);
	}

	return result;
}

/** Step 1.5: Elide redundant FileReads (before same-file FileEdit). Always applied. */
export function elideRedundantFileReads(
	items: (LaneEvent | CollapsedTaskItem)[],
): (LaneEvent | CollapsedTaskItem)[] {
	const result: (LaneEvent | CollapsedTaskItem)[] = [];

	for (let k = 0; k < items.length; k++) {
		const cur = items[k];
		if ('kind' in cur) {
			result.push(cur);
			continue;
		}
		const curType = cur.event.event_type.type;

		// FileRead suppression
		if (curType === 'FileRead' && 'data' in cur.event.event_type) {
			const readPath = (cur.event.event_type.data as { path: string }).path;
			let suppress = false;
			for (let look = k + 1; look < items.length && look <= k + FILEREAD_LOOKAHEAD; look++) {
				const next = items[look];
				if ('kind' in next) break;
				const nextType = next.event.event_type.type;
				if (['UserMessage', 'AgentMessage', 'TaskStart', 'TaskEnd'].includes(nextType)) break;
				if (nextType === 'FileEdit' && 'data' in next.event.event_type) {
					const editPath = (next.event.event_type.data as { path: string }).path;
					if (editPath === readPath) {
						suppress = true;
						break;
					}
				}
			}
			if (suppress) continue;
		}

		result.push(cur);
	}

	return result;
}

/** Maps specialized event types to their corresponding ToolResult names */
const SPECIALIZED_RESULT_NAMES: Record<string, string[]> = {
	FileRead: ['Read'],
	FileEdit: ['Edit', 'Write'],
	FileCreate: ['Write'],
	FileSearch: ['Glob'],
	CodeSearch: ['Grep'],
	ShellCommand: ['Bash'],
	WebSearch: ['WebSearch'],
	WebFetch: ['WebFetch'],
};

/** Step 1.75: Pair ToolCall + ToolResult (and specialized events + ToolResult) when adjacent. */
export function pairToolCallResults(
	items: (LaneEvent | CollapsedTaskItem)[],
): (LaneEvent | CollapsedTaskItem | PairedToolCallItem)[] {
	const result: (LaneEvent | CollapsedTaskItem | PairedToolCallItem)[] = [];

	for (let k = 0; k < items.length; k++) {
		const cur = items[k];
		if ('kind' in cur) {
			result.push(cur);
			continue;
		}

		const curType = cur.event.event_type.type;

		// ToolCall + ToolResult pairing
		if (curType === 'ToolCall' && 'data' in cur.event.event_type) {
			const callName = (cur.event.event_type.data as { name: string }).name;
			const next = k + 1 < items.length ? items[k + 1] : null;
			if (
				next &&
				!('kind' in next) &&
				next.event.event_type.type === 'ToolResult' &&
				'data' in next.event.event_type
			) {
				const resultName = (next.event.event_type.data as { name: string }).name;
				if (resultName === callName) {
					result.push({
						kind: 'paired',
						callEvent: cur,
						resultEvent: next,
						lane: cur.lane,
						activeLanes: cur.activeLanes,
						laneColor: cur.laneColor,
					});
					k++;
					continue;
				}
			}
		}

		// Specialized event (FileRead, ShellCommand, etc.) + ToolResult pairing
		const expectedNames = SPECIALIZED_RESULT_NAMES[curType];
		if (expectedNames) {
			const next = k + 1 < items.length ? items[k + 1] : null;
			if (
				next &&
				!('kind' in next) &&
				next.event.event_type.type === 'ToolResult' &&
				'data' in next.event.event_type
			) {
				const resultName = (next.event.event_type.data as { name: string }).name;
				if (expectedNames.includes(resultName)) {
					result.push({
						kind: 'paired',
						callEvent: cur,
						resultEvent: next,
						lane: cur.lane,
						activeLanes: cur.activeLanes,
						laneColor: cur.laneColor,
					});
					k++;
					continue;
				}
			}
		}

		result.push(cur);
	}

	return result;
}

/** Step 2: Collapse consecutive same-type events on the same lane into groups. */
export function collapseConsecutiveEvents(
	items: (LaneEvent | CollapsedTaskItem | PairedToolCallItem)[],
	groupKeyFn: (event: Event) => string | null,
): DisplayItem[] {
	const result: DisplayItem[] = [];
	let i = 0;

	while (i < items.length) {
		const item = items[i];
		if ('kind' in item) {
			result.push(item);
			i++;
			continue;
		}

		const le = item;
		const groupKey = groupKeyFn(le.event);
		if (!groupKey) {
			result.push(le);
			i++;
			continue;
		}

		const group: LaneEvent[] = [le];
		let j = i + 1;
		while (j < items.length) {
			const next = items[j];
			if ('kind' in next) break;
			if (next.lane !== le.lane) break;
			const nextKey = groupKeyFn(next.event);
			if (nextKey !== groupKey) break;
			group.push(next);
			j++;
		}

		if (group.length > 1) {
			result.push({
				kind: 'consecutive',
				events: group,
				groupKey,
				count: group.length,
				lane: le.lane,
				activeLanes: le.activeLanes,
				laneColor: le.laneColor,
			} satisfies ConsecutiveGroupItem);
		} else {
			result.push(le);
		}
		i = j;
	}

	return result;
}
