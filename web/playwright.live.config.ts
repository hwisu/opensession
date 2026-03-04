import { defineConfig, devices } from '@playwright/test';

function resolveBaseUrl(): string {
	const baseUrl = process.env.OPENSESSION_E2E_WORKER_BASE_URL?.trim();
	if (!baseUrl) {
		throw new Error(
			'missing OPENSESSION_E2E_WORKER_BASE_URL for web live E2E (remote fallback is intentionally disabled)',
		);
	}
	const parsed = new URL(baseUrl);
	const host = parsed.hostname.toLowerCase();
	const allowRemote = /^(1|true|yes|on)$/i.test(process.env.OPENSESSION_E2E_ALLOW_REMOTE ?? '0');
	const localHost = host === 'localhost' || host === '127.0.0.1' || host === '::1';
	if (!allowRemote && !localHost) {
		throw new Error(
			`remote web live E2E target blocked: ${baseUrl}. Set OPENSESSION_E2E_ALLOW_REMOTE=1 for intentional remote runs.`,
		);
	}
	return baseUrl;
}

export default defineConfig({
	globalSetup: './e2e/global-setup.ts',
	testDir: './e2e-live',
	fullyParallel: false,
	forbidOnly: !!process.env.CI,
	retries: 0,
	workers: 1,
	reporter: 'list',
	use: {
		baseURL: resolveBaseUrl(),
		trace: 'retain-on-failure',
		screenshot: 'only-on-failure',
	},
	projects: [
		{
			name: 'chromium',
			use: { ...devices['Desktop Chrome'] },
		},
	],
});
