import { expect, test, type Page } from '@playwright/test';
import { getAdmin, getCapabilities, injectAuth } from './helpers';

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000';
const TEST_PASSWORD = 'testpass99!!';

async function seedMockSessionCookies(page: Page) {
	const secure = BASE_URL.startsWith('https://');
	const domain = new URL(BASE_URL).hostname;
	const now = Math.floor(Date.now() / 1000);
	await page.context().addCookies([
		{
			name: 'opensession_access_token',
			value: 'mock-access',
			domain,
			path: '/api',
			httpOnly: true,
			secure,
			sameSite: 'Lax',
			expires: now + 3600,
		},
		{
			name: 'opensession_refresh_token',
			value: 'mock-refresh',
			domain,
			path: '/api',
			httpOnly: true,
			secure,
			sameSite: 'Lax',
			expires: now + 7 * 24 * 3600,
		},
		{
			name: 'opensession_csrf_token',
			value: 'mock-csrf-token',
			domain,
			path: '/',
			httpOnly: false,
			secure,
			sameSite: 'Lax',
			expires: now + 7 * 24 * 3600,
		},
	]);
}

test.describe('Authentication', () => {
	test('first sign in auto-creates account', async ({ page, request }) => {
		const capabilities = await getCapabilities(request);
		test.skip(!capabilities.auth_enabled, 'Auth API is disabled');

		const suffix = crypto.randomUUID().slice(0, 8);
		const email = `pw-first-${suffix}@e2e.local`;
		const nickname = `pw-${suffix}`;

		await page.goto('/login');
		await page.fill('#login-email', email);
		await page.fill('#login-password', TEST_PASSWORD);
		await page.fill('#login-nickname', nickname);
		await page.getByRole('button', { name: 'Continue' }).click();

		await expect(page).toHaveURL(/\/sessions$/);
		await expect(page.locator('[data-testid="account-menu-trigger"]')).toContainText(`[${nickname}]`);
		await page.locator('[data-testid="account-menu-trigger"]').click();
		await expect(page.locator('[data-testid="account-menu"]')).toBeVisible();
		await expect(page.locator('[data-testid="account-menu-logout"]')).toBeVisible();

		const storedTokens = await page.evaluate(() => ({
			access: localStorage.getItem('opensession_access_token'),
			refresh: localStorage.getItem('opensession_refresh_token'),
		}));
		expect(storedTokens.access).toBeNull();
		expect(storedTokens.refresh).toBeNull();
	});

	test('authenticated user sees session list instead of landing', async ({ page, request }) => {
		const capabilities = await getCapabilities(request);
		test.skip(!capabilities.auth_enabled, 'Auth API is disabled');

		const admin = await getAdmin(request);
		await injectAuth(page, admin);
		await page.goto('/sessions');
		await expect(page.locator('#session-search')).toBeVisible();
		await expect(page.locator('nav')).toBeVisible();
	});

	test('authenticated user sees nickname in nav', async ({ page, request }) => {
		const capabilities = await getCapabilities(request);
		test.skip(!capabilities.auth_enabled, 'Auth API is disabled');

		const admin = await getAdmin(request);
		await injectAuth(page, admin);
		await page.goto('/sessions');
		await expect(page.locator('[data-testid="account-menu-trigger"]')).toContainText(`[${admin.nickname}]`);
		await page.locator('[data-testid="account-menu-trigger"]').click();
		await expect(page.locator('[data-testid="account-menu"]')).toBeVisible();
		await expect(page.locator('[data-testid="account-menu-logout"]')).toBeVisible();
		await expect(page.locator('[data-testid="account-menu"]')).toContainText('User ID:');
	});

	test('authenticated user nav prefers github handle over nickname', async ({ page }) => {
		await seedMockSessionCookies(page);

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

		await page.goto('/sessions');
		await expect(page.locator('[data-testid="account-menu-trigger"]')).toContainText('[@octocat]');
		await expect(page.locator('[data-testid="account-menu-trigger"]')).not.toContainText('[fallback-nick]');
		await page.locator('[data-testid="account-menu-trigger"]').click();
		await expect(page.locator('[data-testid="account-menu"]')).toContainText('Providers:');
		await expect(page.locator('[data-testid="account-menu"]')).toContainText('GitHub');
	});

	test('legacy localStorage auth tokens are ignored', async ({ page, request }) => {
		const capabilities = await getCapabilities(request);
		test.skip(!capabilities.auth_enabled, 'Auth API is disabled');

		await page.goto('/sessions');
		await page.evaluate(() => {
			localStorage.setItem('opensession_access_token', 'legacy-invalid-access');
			localStorage.setItem('opensession_refresh_token', 'legacy-invalid-refresh');
			localStorage.setItem('opensession_token_expiry', String(Date.now() - 60_000));
		});

		await page.goto('/sessions');
		await expect(page.locator('#session-search')).toBeVisible();
		await expect(page.locator('nav').getByText('Login')).toBeVisible();
		await expect(page.locator('[data-testid="account-menu-trigger"]')).toHaveCount(0);
	});

	test('api key alone does not count as logged-in web session', async ({ page, request }) => {
		const capabilities = await getCapabilities(request);
		test.skip(!capabilities.auth_enabled, 'Auth API is disabled');

		await page.goto('/sessions');
		await page.evaluate((apiKey) => {
			localStorage.setItem('opensession_api_key', apiKey);
		}, 'osk_test_only_key');

		await page.goto('/sessions');
		await expect(page.locator('#session-search')).toBeVisible();
		await expect(page.locator('nav').getByText('Login')).toBeVisible();
		await expect(page.locator('[data-testid="account-menu-trigger"]')).toHaveCount(0);
		await expect(page.locator('[data-testid="account-menu-logout"]')).toHaveCount(0);
	});

	test('account dropdown logout signs out and returns to guest nav', async ({ page }) => {
		await seedMockSessionCookies(page);
		let loggedOut = false;

		await page.route('**/api/capabilities', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					auth_enabled: true,
					parse_preview_enabled: false,
					register_targets: ['local', 'git'],
					share_modes: ['web', 'git', 'json'],
				}),
			});
		});
		await page.route('**/api/auth/verify', async (route) => {
			if (loggedOut) {
				await route.fulfill({
					status: 401,
					contentType: 'application/json',
					body: JSON.stringify({ code: 'unauthorized', message: 'logged out' }),
				});
				return;
			}
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({ user_id: 'u-logout', nickname: 'logout-user' }),
			});
		});
		await page.route('**/api/auth/me', async (route) => {
			if (loggedOut) {
				await route.fulfill({
					status: 401,
					contentType: 'application/json',
					body: JSON.stringify({ code: 'unauthorized', message: 'logged out' }),
				});
				return;
			}
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					user_id: 'u-logout',
					nickname: 'logout-user',
					created_at: new Date().toISOString(),
					email: 'logout@test.local',
					avatar_url: null,
					oauth_providers: [],
				}),
			});
		});
		await page.route('**/api/auth/logout', async (route) => {
			loggedOut = true;
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				headers: {
					'set-cookie': 'opensession_csrf_token=; Path=/; Max-Age=0; SameSite=Lax',
				},
				body: JSON.stringify({ ok: true }),
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

		await page.goto('/sessions');
		await page.locator('[data-testid="account-menu-trigger"]').click();
		const layoutWidths = await page.evaluate(() => ({
			scrollWidth: document.documentElement.scrollWidth,
			clientWidth: document.documentElement.clientWidth,
		}));
		expect(layoutWidths.scrollWidth).toBeLessThanOrEqual(layoutWidths.clientWidth + 1);
		await page.locator('[data-testid="account-menu-logout"]').click();
		await expect(page).toHaveURL(/\/sessions$/);
		await expect(page.locator('nav').getByText('Login')).toBeVisible();
		await expect(page.locator('[data-testid="account-menu-trigger"]')).toHaveCount(0);
	});

	test('docs page accessible without auth', async ({ page }) => {
		await page.goto('/docs');
		await expect(page.locator('main')).toBeVisible();
	});
});
