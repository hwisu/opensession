import { ApiError } from '../api-internal/errors';
import type { UserSettings } from '../types';

export interface ShellModelState {
	user: UserSettings | null;
	paletteOpen: boolean;
	paletteQuery: string;
	paletteSelectionIndex: number;
	helpOpen: boolean;
	accountMenuOpen: boolean;
	authEnabled: boolean;
	hasLocalAuth: boolean;
	desktopRuntime: boolean;
}

export interface ShellModelDeps {
	getApiCapabilities: () => Promise<{ auth_enabled: boolean }>;
	verifyAuth: () => Promise<boolean>;
	getSettings: () => Promise<UserSettings>;
	authLogout: () => Promise<void>;
	isAuthenticated: () => boolean;
	isDesktopRuntime: () => boolean;
	takeLaunchRoute: () => Promise<string | null>;
	getCurrentLocationPath: () => string;
	startInterval: (callback: () => void, ms: number) => unknown;
	clearInterval: (handle: unknown) => void;
	navigate: (path: string) => void;
}

export function createShellModelState(): ShellModelState {
	return {
		user: null,
		paletteOpen: false,
		paletteQuery: '',
		paletteSelectionIndex: 0,
		helpOpen: false,
		accountMenuOpen: false,
		authEnabled: false,
		hasLocalAuth: false,
		desktopRuntime: false,
	};
}

export function createShellModel(state: ShellModelState, deps: ShellModelDeps) {
	let capabilityRequestId = 0;
	let userRequestId = 0;

	function clearAuthState() {
		state.user = null;
		state.hasLocalAuth = false;
		state.accountMenuOpen = false;
	}

	async function loadCapabilities() {
		const requestId = ++capabilityRequestId;
		state.desktopRuntime = deps.isDesktopRuntime();
		try {
			const capabilities = await deps.getApiCapabilities();
			if (requestId !== capabilityRequestId) return;
			state.authEnabled = capabilities.auth_enabled;
		} catch {
			if (requestId !== capabilityRequestId) return;
			state.authEnabled = false;
		}
	}

	async function loadUser() {
		const requestId = ++userRequestId;
		if (!state.authEnabled) {
			clearAuthState();
			return;
		}
		if (!deps.isAuthenticated()) {
			clearAuthState();
			return;
		}

		try {
			const verified = await deps.verifyAuth();
			if (requestId !== userRequestId) return;
			if (!verified) {
				clearAuthState();
				return;
			}
			const settings = await deps.getSettings();
			if (requestId !== userRequestId) return;
			state.user = settings;
			state.hasLocalAuth = true;
		} catch (error) {
			if (requestId !== userRequestId) return;
			clearAuthState();
			if (error instanceof ApiError && (error.status === 401 || error.status === 403)) {
				await deps.authLogout();
			}
		}
	}

	function resetMenusForPath() {
		state.accountMenuOpen = false;
	}

	function openPalette() {
		state.paletteOpen = true;
		state.paletteQuery = '';
		state.paletteSelectionIndex = 0;
	}

	function closePalette() {
		state.paletteOpen = false;
		state.paletteQuery = '';
	}

	function openHelp() {
		state.helpOpen = true;
	}

	function closeHelp() {
		state.helpOpen = false;
	}

	function closeAccountMenu() {
		state.accountMenuOpen = false;
	}

	function toggleAccountMenu() {
		state.accountMenuOpen = !state.accountMenuOpen;
	}

	function resetPaletteSelection() {
		state.paletteSelectionIndex = 0;
	}

	function clampPaletteSelection(maxIndex: number) {
		if (maxIndex < 0) {
			state.paletteSelectionIndex = 0;
			return;
		}
		if (state.paletteSelectionIndex > maxIndex) {
			state.paletteSelectionIndex = maxIndex;
		}
	}

	function movePaletteSelection(direction: 1 | -1, visibleCount: number) {
		if (visibleCount <= 0) return;
		state.paletteSelectionIndex =
			(state.paletteSelectionIndex + direction + visibleCount) % visibleCount;
	}

	async function signOut() {
		closeAccountMenu();
		await deps.authLogout();
		clearAuthState();
		deps.navigate('/sessions');
	}

	function startLaunchRoutePolling() {
		let cancelled = false;
		let commandSupported = true;
		let timer: unknown;

		const stop = () => {
			if (timer !== undefined) {
				deps.clearInterval(timer);
				timer = undefined;
			}
		};

		const poll = async () => {
			if (cancelled || !commandSupported) return;
			try {
				const maybeRoute = await deps.takeLaunchRoute();
				if (typeof maybeRoute !== 'string') return;
				const nextPath = maybeRoute.trim();
				if (!nextPath || nextPath === deps.getCurrentLocationPath()) return;
				deps.navigate(nextPath);
			} catch {
				commandSupported = false;
				stop();
			}
		};

		void poll();
		timer = deps.startInterval(() => {
			void poll();
		}, 1200);

		return () => {
			cancelled = true;
			stop();
		};
	}

	return {
		loadCapabilities,
		loadUser,
		resetMenusForPath,
		openPalette,
		closePalette,
		openHelp,
		closeHelp,
		closeAccountMenu,
		toggleAccountMenu,
		resetPaletteSelection,
		clampPaletteSelection,
		movePaletteSelection,
		signOut,
		startLaunchRoutePolling,
	};
}
