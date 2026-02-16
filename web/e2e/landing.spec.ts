import { test, expect } from '@playwright/test';
import { getAdmin, uploadSession } from './helpers';

test.describe('Home Feed (unauthenticated)', () => {
	test('shows session feed controls immediately', async ({ page }) => {
		await page.goto('/');
		await expect(page.locator('#session-search')).toBeVisible();
		await expect(page.getByText('All Time')).toBeVisible();
		await expect(page.locator('select').first()).toBeVisible();
	});

	test('shows unauthenticated banner and login link', async ({ page }) => {
		await page.goto('/');
		await expect(
			page.getByText('You are browsing public/local sessions. Sign in to upload, manage teams, and use inbox.'),
		).toBeVisible();
		await expect(page.getByRole('button', { name: 'Sign in' })).toBeVisible();
		await expect(page.locator('nav').getByText('Login')).toBeVisible();
	});

	test('renders newly uploaded public session without authentication', async ({ page, request }) => {
		const admin = await getAdmin(request);
		const title = `PW Public Feed ${crypto.randomUUID().slice(0, 8)}`;
		await uploadSession(request, admin.access_token, { title });

		await page.goto('/');
		await expect(page.getByText(title)).toBeVisible({ timeout: 10000 });
	});
});
