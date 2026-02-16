export type SourceStatus = 'open-source' | 'closed-core';

export interface ParserConformanceReference {
	label: string;
	url: string;
}

export interface ParserConformanceRow {
	tool: string;
	parser: string;
	sourceStatus: SourceStatus;
	fixtureCount: number;
	criticalChecks: string[];
	lastVerifiedAt: string;
	references: ParserConformanceReference[];
}

export const PARSER_CONFORMANCE_ROWS: ParserConformanceRow[] = [
	{
		tool: 'Codex',
		parser: 'codex',
		sourceStatus: 'open-source',
		fixtureCount: 2,
		criticalChecks: [
			'event_msg token_count and reasoning normalization',
			'response_item web_search action mapping',
			'response_item and event_msg dedupe stability',
		],
		lastVerifiedAt: '2026-02-16',
		references: [
			{ label: 'openai/codex', url: 'https://github.com/openai/codex' },
		],
	},
	{
		tool: 'Claude Code',
		parser: 'claude-code',
		sourceStatus: 'open-source',
		fixtureCount: 1,
		criticalChecks: [
			'tool_use and tool_result fallback pairing',
			'missing tool_use_id recovery',
			'subagent lane/task attribution',
		],
		lastVerifiedAt: '2026-02-16',
		references: [
			{ label: 'anthropics/claude-code', url: 'https://github.com/anthropics/claude-code' },
		],
	},
	{
		tool: 'Cursor',
		parser: 'cursor',
		sourceStatus: 'closed-core',
		fixtureCount: 1,
		criticalChecks: [
			'state.vscdb v3 bubble restore',
			'tool_former_data recovery',
			'TaskStart and TaskEnd lane balancing',
		],
		lastVerifiedAt: '2026-02-16',
		references: [
			{ label: 'cursor/cursor (issue tracker)', url: 'https://github.com/cursor/cursor' },
		],
	},
	{
		tool: 'Gemini CLI',
		parser: 'gemini',
		sourceStatus: 'open-source',
		fixtureCount: 2,
		criticalChecks: [
			'messages content string and part array compatibility',
			'toolCalls and tool response extraction',
			'thinking/progress semantic mapping',
		],
		lastVerifiedAt: '2026-02-16',
		references: [
			{ label: 'google-gemini/gemini-cli', url: 'https://github.com/google-gemini/gemini-cli' },
		],
	},
	{
		tool: 'OpenCode',
		parser: 'opencode',
		sourceStatus: 'open-source',
		fixtureCount: 2,
		criticalChecks: [
			'providerID/modelID shape compatibility',
			'part/msg tree reconstruction',
			'tool call lifecycle pairing',
		],
		lastVerifiedAt: '2026-02-16',
		references: [
			{ label: 'opencode-ai/opencode', url: 'https://github.com/opencode-ai/opencode' },
		],
	},
];

export function conformanceCoverageScore(rows: ParserConformanceRow[]): number {
	if (rows.length === 0) return 0;
	const weighted = rows.reduce((sum, row) => {
		const fixtureWeight = Math.min(3, row.fixtureCount);
		const checkWeight = Math.min(4, row.criticalChecks.length);
		return sum + fixtureWeight + checkWeight;
	}, 0);
	const max = rows.length * (3 + 4);
	return Math.round((weighted / max) * 100);
}
