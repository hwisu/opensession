import { test, expect } from '@playwright/test';

test.describe('API Health', () => {
	test('health endpoint returns ok', async ({ request }) => {
		const resp = await request.get('/api/health');
		expect(resp.ok()).toBeTruthy();
		const body = await resp.json();
		expect(body.status).toBe('ok');
	});

	test('capabilities endpoint returns auth/parse/share fields', async ({ request }) => {
		const resp = await request.get('/api/capabilities');
		expect(resp.ok()).toBeTruthy();
		const body: {
			auth_enabled: boolean;
			parse_preview_enabled: boolean;
			register_targets: string[];
			share_modes: string[];
		} = await resp.json();
		expect(typeof body.auth_enabled).toBe('boolean');
		expect(typeof body.parse_preview_enabled).toBe('boolean');
		expect(Array.isArray(body.register_targets)).toBeTruthy();
		expect(Array.isArray(body.share_modes)).toBeTruthy();
	});

	test('auth providers endpoint returns email_password', async ({ request }) => {
		const resp = await request.get('/api/auth/providers');
		expect(resp.ok()).toBeTruthy();
		const body: { email_password: boolean } = await resp.json();
		expect(typeof body.email_password).toBe('boolean');
	});
});
