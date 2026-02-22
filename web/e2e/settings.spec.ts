import { expect, test } from '@playwright/test';

test.describe('Settings', () => {
	test('settings page requires auth when no token exists', async ({ page }) => {
		await page.goto('/settings');
		await expect(page.locator('[data-testid="settings-require-auth"]')).toBeVisible();
		await expect(page.locator('[data-testid="settings-require-auth"]')).toContainText(
			'Sign in is required',
		);
	});

	test('can issue personal api key from settings page', async ({ page }) => {
		const expiry = Math.floor(Date.now() / 1000) + 3600;
		await page.addInitScript((nextExpiry) => {
			localStorage.setItem('opensession_access_token', 'settings-access');
			localStorage.setItem('opensession_refresh_token', 'settings-refresh');
			localStorage.setItem('opensession_token_expiry', String(nextExpiry));
		}, expiry);

		await page.route('**/api/capabilities', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					auth_enabled: true,
					parse_preview_enabled: true,
					register_targets: ['local', 'git'],
					share_modes: ['web', 'git', 'json'],
				}),
			});
		});

		await page.route('**/api/auth/verify', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({ user_id: 'u-settings', nickname: 'settings-user' }),
			});
		});

		await page.route('**/api/auth/me', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					user_id: 'u-settings',
					nickname: 'settings-user',
					created_at: '2026-02-21T00:00:00.000Z',
					email: 'settings@test.local',
					avatar_url: null,
					oauth_providers: [
						{ provider: 'github', provider_username: 'settings-user', display_name: 'GitHub' },
					],
				}),
			});
		});

		await page.route('**/api/auth/api-keys/issue', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({ api_key: 'osk_settings_e2e_test_key' }),
			});
		});

		await page.goto('/settings');
		await expect(page.locator('[data-testid="settings-page"]')).toBeVisible();
		await expect(page.locator('[data-testid="settings-page"]')).toContainText('settings-user');
		await expect(page.locator('[data-testid="settings-page"]')).toContainText('settings@test.local');

		await page.locator('[data-testid="issue-api-key-button"]').click();
		await expect(page.locator('[data-testid="issued-api-key"]')).toContainText(
			'osk_settings_e2e_test_key',
		);
		await expect(page.locator('[data-testid="copy-api-key"]')).toBeVisible();
	});
});
