import { test, expect } from '@playwright/test';
import { getAdmin, injectAuth } from './helpers';

test.describe('Navigation', () => {
	test('nav links are present', async ({ page }) => {
		await page.goto('/');
		const nav = page.locator('nav');
		await expect(nav.getByText('Docs')).toBeVisible();
		await expect(nav.getByText('Teams')).toBeVisible();
		await expect(nav.getByText('Upload')).toBeVisible();
	});

	test('clicking Docs navigates to /docs', async ({ page }) => {
		await page.goto('/');
		await page.locator('nav').getByText('Docs').click();
		await expect(page).toHaveURL(/\/docs/);
	});

	test('footer shows keyboard hints', async ({ page }) => {
		await page.goto('/');
		await expect(page.locator('footer').getByText('opensession.io')).toBeVisible();
	});

	test('authenticated nav shows settings link', async ({ page, request }) => {
		const admin = await getAdmin(request);
		await injectAuth(page, admin);
		await page.goto('/');
		await expect(
			page.locator('nav').getByText(`[${admin.nickname}]`),
		).toBeVisible();
	});
});
