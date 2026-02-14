import type { Event } from './types';

export interface LaneEvent {
	event: Event;
	lane: number;
	activeLanes: number[];
	isFork: boolean;
	isMerge: boolean;
	forkLane?: number;
	mergeLane?: number;
	laneColor: string;
}

export interface TaskInfo {
	taskId: string;
	title: string;
	purpose: string;
	eventCount: number;
	durationMs: number;
	startedAt: string;
	endedAt: string;
	lane: number;
	activeLanesAtStart: number[];
	activeLanesAtEnd: number[];
}

export type CollapsedTaskItem = {
	kind: 'collapsed';
	taskId: string;
	info: TaskInfo;
	activeLanes: number[];
	lane: number;
};

export type ConsecutiveGroupItem = {
	kind: 'consecutive';
	events: LaneEvent[];
	groupKey: string;
	count: number;
	lane: number;
	activeLanes: number[];
	laneColor: string;
};

export type PairedToolCallItem = {
	kind: 'paired';
	callEvent: LaneEvent;
	resultEvent: LaneEvent;
	lane: number;
	activeLanes: number[];
	laneColor: string;
};

export type DisplayItem = LaneEvent | CollapsedTaskItem | ConsecutiveGroupItem | PairedToolCallItem;
