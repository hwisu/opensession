import { test, expect } from '@playwright/test';
import { getAdmin, isWorkerProfile, uploadSession } from './helpers';

test.describe('Home Feed (unauthenticated)', () => {
	test('shows profile-appropriate home for guests', async ({ page }) => {
		await page.goto('/');
		if (isWorkerProfile) {
			await expect(page.locator('#session-search')).toBeVisible();
			await expect(page.locator('h1').filter({ hasText: 'AI sessions are' })).toHaveCount(0);
		} else {
			await expect(page.locator('h1').filter({ hasText: 'AI sessions are' })).toBeVisible();
			await expect(page.locator('#session-search')).toHaveCount(0);
			await expect(page.getByRole('button', { name: 'Sign In' })).toBeVisible();
		}
		if (isWorkerProfile) {
			await expect(page.locator('nav').getByText('Login')).toHaveCount(0);
		} else {
			await expect(page.locator('nav').getByText('Login')).toBeVisible();
		}
	});

	test('worker guest home requests and renders the public session list', async ({ page }) => {
		test.skip(!isWorkerProfile, 'Worker-only behavior');

		let listRequests = 0;
		await page.route('**/api/sessions**', async (route) => {
			listRequests += 1;
			await route.continue();
		});

		await page.goto('/');
		await expect(page.locator('#session-search')).toBeVisible();
		await page.waitForTimeout(300);
		expect(listRequests).toBeGreaterThan(0);
	});

	test('worker profile redirects /login to /', async ({ page }) => {
		test.skip(!isWorkerProfile, 'Worker-only guest redirect behavior');

		await page.goto('/login');
		await expect(page).toHaveURL(/\/$/);
		await expect(page.locator('#session-search')).toBeVisible();
	});

	test('renders newly uploaded public session without authentication', async ({ page, request }) => {
		const admin = await getAdmin(request);
		const title = `PW Public Feed ${crypto.randomUUID().slice(0, 8)}`;
		const sessionId = await uploadSession(request, admin.access_token, { title });

		if (isWorkerProfile) {
			await page.goto('/');
			await expect(page.getByText(title)).toBeVisible({ timeout: 10000 });

			await page.goto(`/session/${sessionId}`);
			await expect(page).toHaveURL(new RegExp(`/session/${sessionId}$`));
			await expect(page.locator('main').getByText(title)).toBeVisible({ timeout: 10000 });
		} else {
			await page.goto('/');
			await expect(page.locator('h1').filter({ hasText: 'AI sessions are' })).toBeVisible();
			await expect(page.locator('main').getByText(title)).toHaveCount(0);
		}
	});
});
