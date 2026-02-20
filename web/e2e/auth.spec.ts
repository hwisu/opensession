import { test, expect } from '@playwright/test';
import { getAdmin, getCapabilities, injectAuth } from './helpers';

test.describe('Authentication', () => {
	test('first sign in auto-creates account', async ({ page, request }) => {
		const capabilities = await getCapabilities(request);
		test.skip(!capabilities.auth_enabled, 'Auth API is disabled');

		const suffix = crypto.randomUUID().slice(0, 8);
		const email = `pw-first-${suffix}@e2e.local`;
		const password = 'testpass99';
		const nickname = `pw-${suffix}`;

		await page.goto('/login');
		await page.fill('#login-email', email);
		await page.fill('#login-password', password);
		await page.fill('#login-nickname', nickname);
		await page.getByRole('button', { name: 'Continue' }).click();

		await expect(page).toHaveURL(/\/$/);
		await expect(page.locator('nav').getByText(`[${nickname}]`)).toBeVisible();
		await expect(page.locator('nav').getByText('Logout')).toBeVisible();
	});

	test('authenticated user sees session list instead of landing', async ({ page, request }) => {
		const capabilities = await getCapabilities(request);
		test.skip(!capabilities.auth_enabled, 'Auth API is disabled');

		const admin = await getAdmin(request);
		await injectAuth(page, admin);
		await page.goto('/');
		await expect(page.locator('#session-search')).toBeVisible();
		await expect(page.locator('nav')).toBeVisible();
	});

	test('authenticated user sees nickname in nav', async ({ page, request }) => {
		const capabilities = await getCapabilities(request);
		test.skip(!capabilities.auth_enabled, 'Auth API is disabled');

		const admin = await getAdmin(request);
		await injectAuth(page, admin);
		await page.goto('/');
		await expect(page.locator('nav').getByText(`[${admin.nickname}]`)).toBeVisible();
		await expect(page.locator('nav').getByText('Logout')).toBeVisible();
	});

	test('authenticated user nav prefers github handle over nickname', async ({ page }) => {
		const expiry = Math.floor(Date.now() / 1000) + 3600;
		await page.addInitScript((nextExpiry) => {
			localStorage.setItem('opensession_access_token', 'test-access');
			localStorage.setItem('opensession_refresh_token', 'test-refresh');
			localStorage.setItem('opensession_token_expiry', String(nextExpiry));
		}, expiry);

		await page.route('**/api/capabilities', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					auth_enabled: true,
					upload_enabled: true,
					ingest_preview_enabled: true,
					gh_share_enabled: true,
				}),
			});
		});
		await page.route('**/api/auth/verify', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({ user_id: 'u-test', nickname: 'fallback-nick' }),
			});
		});
		await page.route('**/api/auth/me', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					user_id: 'u-test',
					nickname: 'fallback-nick',
					created_at: new Date().toISOString(),
					email: null,
					avatar_url: null,
					oauth_providers: [
						{ provider: 'github', provider_username: '@octocat', display_name: 'GitHub' },
					],
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
		await expect(page.locator('nav').getByText('[@octocat]')).toBeVisible();
		await expect(page.locator('nav').getByText('[fallback-nick]')).toHaveCount(0);
	});

	test('legacy millisecond token expiry does not keep guest in authenticated home', async ({
		page, request,
	}) => {
		const capabilities = await getCapabilities(request);
		test.skip(!capabilities.auth_enabled, 'Auth API is disabled');

		await page.goto('/');
		await page.evaluate(() => {
			localStorage.setItem('opensession_access_token', 'legacy-invalid-access');
			localStorage.setItem('opensession_refresh_token', 'legacy-invalid-refresh');
			localStorage.setItem('opensession_token_expiry', String(Date.now() - 60_000));
		});

		await page.goto('/');
		await expect(page.locator('#session-search')).toBeVisible();
		await expect(page.locator('nav').getByText('Login')).toBeVisible();

		const tokens = await page.evaluate(() => ({
			access: localStorage.getItem('opensession_access_token'),
			refresh: localStorage.getItem('opensession_refresh_token'),
		}));
		expect(tokens.access).toBeNull();
		expect(tokens.refresh).toBeNull();
	});

	test('api key alone does not count as logged-in web session', async ({ page, request }) => {
		const capabilities = await getCapabilities(request);
		test.skip(!capabilities.auth_enabled, 'Auth API is disabled');

		await page.goto('/');
		await page.evaluate((apiKey) => {
			localStorage.removeItem('opensession_access_token');
			localStorage.removeItem('opensession_refresh_token');
			localStorage.removeItem('opensession_token_expiry');
			localStorage.setItem('opensession_api_key', apiKey);
		}, 'osk_test_only_key');

		await page.goto('/');
		await expect(page.locator('#session-search')).toBeVisible();
		await expect(page.locator('nav').getByText('Login')).toBeVisible();
		await expect(page.locator('nav').getByText('Logout')).toHaveCount(0);
	});

	test('docs page accessible without auth', async ({ page }) => {
		await page.goto('/docs');
		await expect(page.locator('main')).toBeVisible();
	});
});
