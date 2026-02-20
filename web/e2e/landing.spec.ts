import { test, expect } from '@playwright/test';
import { getAdmin, getCapabilities, uploadSession } from './helpers';

test.describe('Landing (unauthenticated)', () => {
	test('shows landing sections and nav for guests, without inline session list', async ({ page }) => {
		await page.goto('/');
		await expect(page.locator('nav').getByText('Sessions')).toBeVisible();
		await expect(
			page.locator('h1').filter({ hasText: 'Track real AI coding sessions with one consistent data model.' }),
		).toBeVisible();
		await expect(page.locator('#session-search')).toHaveCount(0);
		await expect(page.locator('nav').getByText('Login')).toBeVisible();
		await expect(page.locator('nav').getByText('Register')).toHaveCount(0);
	});

	test('landing renders feature map, data flow, and capability matrix', async ({ page }) => {
		await page.goto('/');
		await expect(page.locator('[data-contract-section="feature-map"]')).toBeVisible();
		await expect(page.locator('[data-contract-section="data-flow"]')).toBeVisible();
		await expect(page.locator('[data-contract-section="capability-matrix"]')).toBeVisible();
	});

	test('capability matrix explains mixed runtime flags', async ({ page }) => {
		await page.route('**/api/capabilities', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					auth_enabled: true,
					upload_enabled: false,
					ingest_preview_enabled: false,
					gh_share_enabled: false,
				}),
			});
		});
		await page.goto('/');

		const matrix = page.locator('[data-contract-section="capability-matrix"]');
		await expect(matrix.locator('[data-capability-key="auth_enabled"]')).toContainText('Available');
		await expect(matrix.locator('[data-capability-key="auth_enabled"]')).toContainText('owner write protection');
		await expect(matrix.locator('[data-capability-key="upload_enabled"]')).toContainText('Disabled');
		await expect(matrix.locator('[data-capability-key="upload_enabled"]')).toContainText('read-only');
		await expect(matrix.locator('[data-capability-key="gh_share_enabled"]')).toContainText('unavailable');
		await expect(matrix).toContainText('Flags are independent.');
	});

	test('landing quick action opens docs page', async ({ page }) => {
		await page.goto('/');
		await page.getByRole('button', { name: 'Open Docs' }).click();
		await expect(page).toHaveURL(/\/docs$/);
		await expect(page.locator('main h1').first()).toContainText('Documentation');
	});

	test('login page is accessible to guests', async ({ page }) => {
		const resp = await page.request.get('/api/capabilities');
		expect(resp.ok()).toBeTruthy();
		const capabilities: { auth_enabled: boolean } = await resp.json();

		await page.goto('/login');
		await expect(page).toHaveURL(/\/login$/);
		if (capabilities.auth_enabled) {
			await expect(page.locator('#login-email')).toBeVisible();
			await expect(page.locator('#login-nickname')).toBeVisible();
		} else {
			await expect(page.locator('[data-testid="auth-unavailable"]')).toBeVisible();
		}
	});

	test('register path redirects to login', async ({ page }) => {
		await page.goto('/register');
		await expect(page).toHaveURL(/\/login$/);
	});

	test('renders newly uploaded public session without authentication', async ({ page, request }) => {
		const capabilities = await getCapabilities(request);
		test.skip(!capabilities.auth_enabled, 'Auth API is disabled');
		test.skip(!capabilities.upload_enabled, 'Upload API is disabled');

		const admin = await getAdmin(request);
		const title = `PW Public Feed ${crypto.randomUUID().slice(0, 8)}`;
		const sessionId = await uploadSession(request, admin.access_token, { title });

		await page.goto('/sessions');
		await expect(page.locator('#session-search')).toBeVisible();
		await expect(page.locator('main').getByText(title)).toBeVisible({ timeout: 10000 });

		await page.goto(`/session/${sessionId}`);
		await expect(page).toHaveURL(new RegExp(`/session/${sessionId}$`));
		await expect(page.locator('main').getByText(title)).toBeVisible({ timeout: 10000 });
	});
});
