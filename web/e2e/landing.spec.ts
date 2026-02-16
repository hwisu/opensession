import { test, expect } from '@playwright/test';
import { getAdmin, isWorkerProfile, uploadSession } from './helpers';

test.describe('Home Feed (unauthenticated)', () => {
	test('shows profile-appropriate home for guests', async ({ page }) => {
		await page.goto('/');
		if (isWorkerProfile) {
			await expect(page.locator('h1').filter({ hasText: 'AI sessions are' })).toBeVisible();
			await expect(page.locator('#session-search')).toHaveCount(0);
		} else {
			await expect(page.locator('#session-search')).toBeVisible();
			await expect(page.getByText('All Time')).toBeVisible();
			await expect(page.locator('select').first()).toBeVisible();
			await expect(
				page.getByText(
					'You are browsing public/local sessions. Sign in to upload, manage teams, and use inbox.',
				),
			).toBeVisible();
			await expect(page.getByRole('button', { name: 'Sign in' })).toBeVisible();
		}
		await expect(page.locator('nav').getByText('Login')).toBeVisible();
	});

	test('worker guest home does not request session list before rendering landing', async ({ page }) => {
		test.skip(!isWorkerProfile, 'Worker-only landing behavior');

		let listRequests = 0;
		await page.route('**/api/sessions**', async (route) => {
			listRequests += 1;
			await route.continue();
		});

		await page.goto('/');
		await expect(page.locator('h1').filter({ hasText: 'AI sessions are' })).toBeVisible();
		await expect(page.locator('#session-search')).toHaveCount(0);
		await page.waitForTimeout(300);
		expect(listRequests).toBe(0);
	});

	test('worker auth guard keeps allowed route when stale check resolves later', async ({ page }) => {
		test.skip(!isWorkerProfile, 'Worker-only guest redirect behavior');

		await page.addInitScript(() => {
			localStorage.setItem('opensession_access_token', 'pw-expired-access');
			localStorage.setItem('opensession_refresh_token', 'pw-expired-refresh');
			localStorage.setItem(
				'opensession_token_expiry',
				String(Math.floor(Date.now() / 1000) - 120),
			);
		});

		await page.route('**/api/auth/refresh', async (route) => {
			await new Promise((resolve) => setTimeout(resolve, 200));
			await route.continue();
		});

		const refreshStarted = page.waitForRequest((request) => request.url().includes('/api/auth/refresh'));
		await page.goto('/upload');
		await refreshStarted;

		await page.goto('/login');
		await expect(page).toHaveURL(/\/login$/);
		await expect(page.getByRole('heading', { name: 'Sign In' })).toBeVisible();
	});

	test('renders newly uploaded public session without authentication', async ({ page, request }) => {
		const admin = await getAdmin(request);
		const title = `PW Public Feed ${crypto.randomUUID().slice(0, 8)}`;
		const sessionId = await uploadSession(request, admin.access_token, { title });

		if (isWorkerProfile) {
			await page.goto(`/session/${sessionId}`);
			await expect(page).toHaveURL(/\/$/);
			await expect(page.locator('h1').filter({ hasText: 'AI sessions are' })).toBeVisible();
			await expect(page.locator('main').getByText(title)).toHaveCount(0);
		} else {
			await page.goto('/');
			await expect(page.getByText(title)).toBeVisible({ timeout: 10000 });
		}
	});
});
