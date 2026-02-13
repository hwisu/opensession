import { test, expect } from '@playwright/test';

test.describe('API Health', () => {
	test('health endpoint returns ok', async ({ request }) => {
		const resp = await request.get('/api/health');
		expect(resp.ok()).toBeTruthy();
		const body = await resp.json();
		expect(body.status).toBe('ok');
	});

	test('auth providers endpoint returns email_password', async ({ request }) => {
		const resp = await request.get('/api/auth/providers');
		expect(resp.ok()).toBeTruthy();
		const body = await resp.json();
		expect(body.email_password).toBe(true);
	});
});
