import { expect, test } from '@playwright/test';

test.describe('Live Auth Flow', () => {
	test('login flow reaches sessions page', async ({ page, request }) => {
		const capabilitiesResponse = await request.get('/api/capabilities');
		expect(capabilitiesResponse.ok()).toBeTruthy();
		const capabilities = (await capabilitiesResponse.json()) as { auth_enabled?: boolean };
		test.skip(!capabilities.auth_enabled, 'Auth API is disabled');

		const suffix = crypto.randomUUID().slice(0, 8);
		const email = `live-${suffix}@e2e.local`;
		const password = 'testpass99!!';
		const nickname = `live-${suffix}`;

		await page.goto('/login');
		await page.fill('#login-email', email);
		await page.fill('#login-password', password);
		await page.fill('#login-nickname', nickname);
		await page.getByRole('button', { name: 'Continue' }).click();

		await expect(page).toHaveURL(/\/sessions$/);
		await expect(page.locator('#session-search')).toBeVisible();
	});
});
