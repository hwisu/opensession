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
	listGroups,
	getGroup,
	createGroup,
	getInviteInfo,
	joinInvite,
	register,
	getSettings,
	regenerateApiKey
} from '@opensession/ui';
