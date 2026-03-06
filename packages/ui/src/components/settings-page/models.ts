export type SettingsSectionNavItem = {
	id: string;
	label: string;
	detail: string;
};

export type RuntimeQuickJumpLink = {
	id: string;
	label: string;
};

export type RuntimeActivityTone = 'enabled' | 'disabled' | 'running' | 'failed' | 'complete';

export type RuntimeActivityBadge = {
	label: string;
	tone: RuntimeActivityTone;
};

export type RuntimeActivityCard = {
	testId: string;
	title: string;
	subtitle: string;
	badges: RuntimeActivityBadge[];
	lines: string[];
	timestampLine: string;
};
