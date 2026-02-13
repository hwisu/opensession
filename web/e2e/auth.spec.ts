import { test, expect } from '@playwright/test';
import { getAdmin, registerUser, injectAuth } from './helpers';

test.describe('Authentication', () => {
	test('register a new user via API', async ({ request }) => {
		const user = await registerUser(request);
		expect(user.access_token).toBeTruthy();
		expect(user.api_key).toBeTruthy();
		expect(user.nickname).toMatch(/^pw-/);
	});

	test('authenticated user sees session list instead of landing', async ({ page, request }) => {
		const admin = await getAdmin(request);
		await injectAuth(page, admin);
		await page.goto('/');
		// Should NOT see landing page hero
		await expect(page.locator('h1').filter({ hasText: 'AI sessions are' })).not.toBeVisible();
		// Should see the nav bar
		await expect(page.locator('nav')).toBeVisible();
	});

	test('authenticated user sees nickname in nav', async ({ page, request }) => {
		const admin = await getAdmin(request);
		await injectAuth(page, admin);
		await page.goto('/');
		await expect(page.locator('nav').getByText(`[${admin.nickname}]`)).toBeVisible();
	});

	test('docs page accessible without auth', async ({ page }) => {
		await page.goto('/docs');
		await expect(page.locator('main')).toBeVisible();
	});
});
