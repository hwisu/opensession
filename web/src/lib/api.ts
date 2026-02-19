// Re-export everything from @opensession/ui
// This shim allows existing $lib/api imports to work
export {
	setApiKey,
	clearApiKey,
	setBaseUrl,
	ApiError,
	isAuthApiAvailable,
	isAuthenticated,
	verifyAuth,
	authLogin,
	authRegister,
	authLogout,
	getSettings,
	getAuthProviders,
	getOAuthUrl,
	handleAuthCallback,
	listSessions,
	getSession,
	uploadSession,
} from '@opensession/ui';
