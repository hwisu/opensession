import { test, expect } from '@playwright/test';
import { createSessionFixture, mockCapabilities, mockSessionApis } from './helpers';

test.describe('Navigation', () => {
	test('unauthenticated nav links are present', async ({ page }) => {
		await page.goto('/');
		const nav = page.locator('nav');
		await expect(nav.getByText('Sessions')).toBeVisible();
		await expect(nav.getByText('Docs')).toBeVisible();
		await expect(nav.getByText('Login')).toBeVisible();
		await expect(nav.getByText('Register')).toHaveCount(0);
		await expect(nav.getByText('DX')).toHaveCount(0);
		await expect(nav.getByText('Inbox')).toHaveCount(0);
		await expect(nav.getByText('Teams')).toHaveCount(0);
		await expect(nav.getByText('Upload')).toHaveCount(0);
	});

	test('session list uses single-feed layout only', async ({ page }) => {
		await page.goto('/sessions');
		await expect(page.locator('[data-testid="list-shortcut-legend"]')).toBeVisible();
		await expect(page.getByRole('tab', { name: 'List' })).toHaveCount(0);
		await expect(page.getByRole('tab', { name: 'Agents' })).toHaveCount(0);
		await expect(page.locator('[data-testid="session-layout-summary"]')).toContainText(
			'single chronological feed',
		);
		await expect(page.locator('[data-testid="session-layout-summary"]')).not.toContainText('grouped by max active agents');
		await expect(page.locator('[data-testid="list-shortcut-legend"]')).not.toContainText('layout');
	});

	test('clicking Docs navigates to /docs', async ({ page }) => {
		await page.goto('/');
		await page.locator('nav').getByText('Docs').click();
		await expect(page).toHaveURL(/\/docs/);
	});

	test('root rem baseline is increased for readability', async ({ page }) => {
		await page.goto('/');
		await expect
			.poll(async () =>
				page.evaluate(() => Number.parseFloat(getComputedStyle(document.documentElement).fontSize)),
			)
			.toBeGreaterThanOrEqual(19);
	});

	test('footer shows keyboard hints', async ({ page }) => {
		await page.goto('/sessions');
		const footer = page.locator('[data-testid="shortcut-footer"]');
		await expect(footer).toBeVisible();
		await expect(footer.getByText('Shortcuts')).toBeVisible();
		expect(await footer.locator('kbd').filter({ hasText: 'Cmd/Ctrl+K' }).count()).toBeGreaterThan(0);
		await expect(footer.getByText('Cmd/Ctrl+K palette')).toBeVisible();
		await expect(footer.getByText('/ search')).toBeVisible();
		await expect(footer.locator('[data-testid="tor-footer-hint"]')).toBeVisible();
		await expect(footer).toContainText('t');
		await expect(footer).toContainText('tool');
		await expect(footer).toContainText('o');
		await expect(footer).toContainText('order');
		await expect(footer).toContainText('r');
		await expect(footer).toContainText('range');
		await expect(footer.getByText('opensession.io')).toBeVisible();
	});

	test('command palette opens and navigates to docs', async ({ page }) => {
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
		await input.fill('docs');
		await page.keyboard.press('Enter');
		await expect(page).toHaveURL(/\/docs/);
	});

	test('session detail footer shows in-session shortcut hints', async ({ page }) => {
		const fixture = createSessionFixture({
			title: `PW Footer Hints ${crypto.randomUUID().slice(0, 8)}`,
		});
		await mockSessionApis(page, fixture, { include_in_list: false });

		await page.goto(`/session/${fixture.id}`);
		const footer = page.locator('[data-testid="shortcut-footer"]');
		await expect(footer).toBeVisible();
		await expect(footer.getByText('Cmd/Ctrl+K palette')).toBeVisible();
		await expect(footer.getByText('/ search')).toBeVisible();
		await expect(footer.getByText('n/p match')).toBeVisible();
	});

	test('authenticated nav shows account dropdown actions', async ({ page }) => {
		const expiry = Math.floor(Date.now() / 1000) + 3600;
		await page.addInitScript((nextExpiry) => {
			localStorage.setItem('opensession_access_token', 'nav-access');
			localStorage.setItem('opensession_refresh_token', 'nav-refresh');
			localStorage.setItem('opensession_token_expiry', String(nextExpiry));
		}, expiry);

		await mockCapabilities(page, { auth_enabled: true, parse_preview_enabled: true });
		await page.route('**/api/auth/verify', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({ user_id: 'u-nav', nickname: 'nav-user' }),
			});
		});
		await page.route('**/api/auth/me', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					user_id: 'u-nav',
					nickname: 'nav-user',
					created_at: new Date().toISOString(),
					email: 'nav@test.local',
					avatar_url: null,
					oauth_providers: [{ provider: 'github', provider_username: 'nav-user', display_name: 'GitHub' }],
				}),
			});
		});
		await page.route('**/api/sessions**', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					sessions: [],
					total: 0,
					page: 1,
					per_page: 50,
				}),
			});
		});

		await page.goto('/');

		await expect(page.locator('[data-testid="account-menu-trigger"]')).toContainText('[@nav-user]');
		await page.locator('[data-testid="account-menu-trigger"]').click();
		const menu = page.locator('[data-testid="account-menu"]');
		await expect(menu).toBeVisible();
		await expect(menu).toContainText('Account');
		await expect(menu).toContainText('nav@test.local');
			await expect(menu).toContainText('Providers:');
			await expect(menu).toContainText('GitHub');
			await expect(menu).toContainText('Settings');
			await expect(menu).toContainText('Session Home');
			await expect(menu).toContainText('Docs');
			await expect(menu.locator('[data-testid="account-menu-logout"]')).toBeVisible();

		const teamsCount = await page.locator('nav').getByText('Teams').count();
		const inboxCount = await page.locator('nav').getByText('Inbox').count();
		if (teamsCount > 0 || inboxCount > 0) {
			await expect(page.locator('nav').getByText('Teams')).toBeVisible();
			await expect(page.locator('nav').getByText('Inbox')).toBeVisible();
		} else {
			await expect(page.locator('nav').getByText('Teams')).toHaveCount(0);
			await expect(page.locator('nav').getByText('Inbox')).toHaveCount(0);
		}
		await expect(menu).not.toContainText('Upload');
	});
});
