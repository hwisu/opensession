export type AppProfile = 'server' | 'worker';

function resolveProfile(value: string | undefined): AppProfile {
	return value?.trim().toLowerCase() === 'worker' ? 'worker' : 'server';
}

export const appProfile: AppProfile = resolveProfile(import.meta.env.VITE_APP_PROFILE);
export const isWorkerProfile = appProfile === 'worker';
export const isServerProfile = appProfile === 'server';
