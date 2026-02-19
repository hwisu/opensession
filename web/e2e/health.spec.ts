import { test, expect } from '@playwright/test';

test.describe('API Health', () => {
	test('health endpoint returns ok', async ({ request }) => {
		const resp = await request.get('/api/health');
		expect(resp.ok()).toBeTruthy();
		const body = await resp.json();
		expect(body.status).toBe('ok');
	});

	test('capabilities endpoint returns auth/upload booleans', async ({ request }) => {
		const resp = await request.get('/api/capabilities');
		expect(resp.ok()).toBeTruthy();
		const body: { auth_enabled: boolean; upload_enabled: boolean } = await resp.json();
		expect(typeof body.auth_enabled).toBe('boolean');
		expect(typeof body.upload_enabled).toBe('boolean');
	});

	test('auth providers endpoint returns email_password', async ({ request }) => {
		const resp = await request.get('/api/auth/providers');
		expect(resp.ok()).toBeTruthy();
		const body: { email_password: boolean } = await resp.json();
		expect(typeof body.email_password).toBe('boolean');
	});
});
