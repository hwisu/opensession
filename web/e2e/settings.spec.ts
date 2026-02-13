import { test, expect } from '@playwright/test';
import { getAdmin, injectAuth } from './helpers';

test.describe('Settings', () => {
	test('settings page loads for authenticated user', async ({ page, request }) => {
		const admin = await getAdmin(request);
		await injectAuth(page, admin);
		await page.goto('/settings');
		await expect(page.locator('main')).toBeVisible();
	});

	test('settings page shows user nickname', async ({ page, request }) => {
		const admin = await getAdmin(request);
		await injectAuth(page, admin);
		await page.goto('/settings');
		// Use exact match to avoid matching both nickname and email
		await expect(
			page.locator('main').getByText(admin.nickname, { exact: true }),
		).toBeVisible({ timeout: 10000 });
	});
});
