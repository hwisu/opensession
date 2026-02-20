import { test, expect } from '@playwright/test';
import { getAdmin, getCapabilities, uploadSession } from './helpers';

test.describe('Home Feed (unauthenticated)', () => {
	test('shows landing and auth nav for guests', async ({ page }) => {
		await page.goto('/');
		await expect(
			page.locator('h1').filter({ hasText: 'AI sessions become a reusable engineering asset.' }),
		).toBeVisible();
		await expect(page.locator('#session-search')).toHaveCount(0);
		await expect(page.getByRole('button', { name: 'Sign In' })).toBeVisible();
		await expect(page.locator('nav').getByText('Login')).toBeVisible();
		await expect(page.locator('nav').getByText('Register')).toHaveCount(0);
	});

	test('landing visualizes feature map, data flow, and capability matrix', async ({ page }) => {
		await page.goto('/');

		await expect(page.locator('[data-contract-section="feature-map"]')).toBeVisible();
		await expect(page.locator('[data-contract-section="data-flow"]')).toBeVisible();
		await expect(page.locator('[data-contract-section="capability-matrix"]')).toBeVisible();

		await expect(page.locator('[data-feature-id]')).toHaveCount(4);
		await expect(page.locator('[data-flow-step]')).toHaveCount(4);

		await expect(page.locator('[data-capability-key="auth_enabled"]')).toBeVisible();
		await expect(page.locator('[data-capability-key="upload_enabled"]')).toBeVisible();
		await expect(page.locator('[data-capability-key="ingest_preview_enabled"]')).toBeVisible();
		await expect(page.locator('[data-capability-key="gh_share_enabled"]')).toBeVisible();
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
		test.skip(!capabilities.upload_enabled, 'Upload API is disabled');

		const admin = await getAdmin(request);
		const title = `PW Public Feed ${crypto.randomUUID().slice(0, 8)}`;
		const sessionId = await uploadSession(request, admin.access_token, { title });

		await page.goto('/');
		await expect(
			page.locator('h1').filter({ hasText: 'AI sessions become a reusable engineering asset.' }),
		).toBeVisible();
		await expect(page.locator('main').getByText(title)).toHaveCount(0);

		await page.goto(`/session/${sessionId}`);
		await expect(page).toHaveURL(new RegExp(`/session/${sessionId}$`));
		await expect(page.locator('main').getByText(title)).toBeVisible({ timeout: 10000 });
	});
});
