// Re-export everything from @opensession/ui
// This shim allows existing $lib/api imports to work
export {
	setApiKey,
	clearApiKey,
	setBaseUrl,
	ApiError,
	listSessions,
	getSession,
	uploadSession,
	listTeams,
	getTeam,
	createTeam,
	updateTeam,
	listMembers,
	addMember,
	removeMember,
	register,
	getSettings,
	regenerateApiKey
} from '@opensession/ui';
