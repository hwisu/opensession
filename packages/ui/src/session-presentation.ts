import type { SessionListItem } from './types';

const GROUP_COLORS = [
	'var(--color-text-secondary)',
	'var(--color-accent)',
	'var(--color-success)',
	'var(--color-warning)',
	'var(--color-tool-amp)',
] as const;

function normalizedText(value: string | null | undefined): string | null {
	if (!value) return null;
	const trimmed = value.trim();
	return trimmed.length > 0 ? trimmed : null;
}

export function getSessionModelLabel(session: SessionListItem): string {
	const model = normalizedText(session.agent_model);
	if (model && model.toLowerCase() !== 'unknown') {
		return model;
	}
	if (session.tool.toLowerCase() === 'codex') {
		return 'codex';
	}
	return '-';
}

export function getSessionActorLabel(session: SessionListItem): string | null {
	const nickname = normalizedText(session.nickname);
	if (nickname) {
		return `@${nickname}`;
	}
	const userId = normalizedText(session.user_id);
	if (!userId) {
		return null;
	}
	return `id:${userId.slice(0, 10)}`;
}

export function getSessionActiveAgentCount(session: SessionListItem): number {
	const raw = Number(session.max_active_agents ?? 1);
	if (!Number.isFinite(raw)) return 1;
	return Math.max(1, Math.trunc(raw));
}

export function formatAgentCountLabel(count: number): string {
	return count === 1 ? '1 agent' : `${count} agents`;
}

export interface SessionAgentGroup {
	count: number;
	label: string;
	color: string;
	sessions: SessionListItem[];
}

export function groupSessionsByAgentCount(sessions: SessionListItem[]): SessionAgentGroup[] {
	const grouped = new Map<number, SessionListItem[]>();
	for (const session of sessions) {
		const count = getSessionActiveAgentCount(session);
		const bucket = grouped.get(count);
		if (bucket) {
			bucket.push(session);
		} else {
			grouped.set(count, [session]);
		}
	}

	return [...grouped.entries()]
		.sort((a, b) => b[0] - a[0])
		.map(([count, rows], idx) => ({
			count,
			label: formatAgentCountLabel(count),
			color: GROUP_COLORS[Math.min(idx, GROUP_COLORS.length - 1)] ?? GROUP_COLORS[0],
			sessions: rows,
		}));
}
