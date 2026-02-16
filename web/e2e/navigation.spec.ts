import { test, expect } from '@playwright/test';
import { getAdmin, injectAuth, uploadSession } from './helpers';

test.describe('Navigation', () => {
	test('unauthenticated nav links are present', async ({ page }) => {
		await page.goto('/');
		const nav = page.locator('nav');
		await expect(nav.getByText('Sessions')).toBeVisible();
		await expect(nav.getByText('DX')).toBeVisible();
		await expect(nav.getByText('Docs')).toBeVisible();
		await expect(nav.getByText('Inbox')).toHaveCount(0);
		await expect(nav.getByText('Teams')).toHaveCount(0);
		await expect(nav.getByText('Upload')).toHaveCount(0);
	});

	test('clicking Docs navigates to /docs', async ({ page }) => {
		await page.goto('/');
		await page.locator('nav').getByText('Docs').click();
		await expect(page).toHaveURL(/\/docs/);
	});

	test('footer shows keyboard hints', async ({ page }) => {
		await page.goto('/');
		const footer = page.locator('[data-testid="shortcut-footer"]');
		await expect(footer).toBeVisible();
		await expect(footer.getByText('Shortcuts')).toBeVisible();
		await expect(footer.getByText('Cmd/Ctrl+K palette')).toBeVisible();
		await expect(footer.getByText('/ search')).toBeVisible();
		await expect(footer.getByText('opensession.io')).toBeVisible();
	});

	test('command palette opens and navigates to DX lab', async ({ page }) => {
		await page.goto('/');
		await expect(page.locator('[data-testid="shortcut-footer"]')).toBeVisible();
		await page.evaluate(() => {
			window.dispatchEvent(
				new KeyboardEvent('keydown', {
					key: 'k',
					ctrlKey: true,
					bubbles: true,
				}),
			);
		});
		const palette = page.locator('[data-testid="command-palette"]');
		await expect(palette).toBeVisible();
		const input = page.locator('[data-testid="command-palette-input"]');
		await expect(input).toBeFocused();
		await input.fill('dx lab');
		await page.keyboard.press('Enter');
		await expect(page).toHaveURL(/\/dx/);
	});

	test('session detail footer shows in-session shortcut hints', async ({ page, request }) => {
		const admin = await getAdmin(request);
		const sessionId = await uploadSession(request, admin.access_token, {
			title: `PW Footer Hints ${crypto.randomUUID().slice(0, 8)}`,
		});

		await injectAuth(page, admin);
		await page.goto(`/session/${sessionId}`);
		const footer = page.locator('[data-testid="shortcut-footer"]');
		await expect(footer).toBeVisible();
		await expect(footer.getByText('Cmd/Ctrl+K palette')).toBeVisible();
		await expect(footer.getByText('/ search')).toBeVisible();
		await expect(footer.getByText('n/p match')).toBeVisible();
	});

	test('authenticated nav shows settings link', async ({ page, request }) => {
		const admin = await getAdmin(request);
		await injectAuth(page, admin);
		await page.goto('/');
		await expect(
			page.locator('nav').getByText(`[${admin.nickname}]`),
		).toBeVisible();
		const teamsCount = await page.locator('nav').getByText('Teams').count();
		const inboxCount = await page.locator('nav').getByText('Inbox').count();
		if (teamsCount > 0 || inboxCount > 0) {
			await expect(page.locator('nav').getByText('Teams')).toBeVisible();
			await expect(page.locator('nav').getByText('Inbox')).toBeVisible();
		} else {
			await expect(page.locator('nav').getByText('Teams')).toHaveCount(0);
			await expect(page.locator('nav').getByText('Inbox')).toHaveCount(0);
		}
		await expect(page.locator('nav').getByText('Upload')).toBeVisible();
	});
});
