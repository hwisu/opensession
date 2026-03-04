import { expect, test } from '@playwright/test';

const REVIEW_ID = 'live-local-review-e2e-1';
const serverApiBaseUrl = process.env.OPENSESSION_E2E_SERVER_BASE_URL?.trim();

test.describe('Live Local Review Route', () => {
	test('renders review bundle data on /review/local/:id', async ({ page }) => {
		test.skip(
			!serverApiBaseUrl,
			'Set OPENSESSION_E2E_SERVER_BASE_URL to run review/local live route against real API',
		);

		await page.goto('/');
		await page.evaluate((apiUrl) => {
			localStorage.setItem('opensession_api_url', apiUrl);
		}, serverApiBaseUrl!);

		await page.goto(`/review/local/${REVIEW_ID}`);

		await expect(page.getByText('PR #7 acme/private-repo')).toBeVisible({ timeout: 10000 });
		await expect(page.getByText('Reviewer Quick Digest')).toBeVisible();
		await expect(page.getByText('What should we verify first?')).toBeVisible();
		await expect(
			page.getByText('Check /review/local/:id digest panel and mapped session render.'),
		).toBeVisible();
		await expect(
			page
				.locator('div.rounded.border.border-border\\/70.p-2.text-xs')
				.filter({ hasText: 'Added/Updated Tests' })
				.getByText('web/e2e-live/live-review-local.spec.ts'),
		).toBeVisible();
		await expect(page.getByText('feat: review local route live test')).toBeVisible();
		await expect(page.getByRole('heading', { name: 'Live Review Session' })).toBeVisible();
	});
});
