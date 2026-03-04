import { expect, test } from '@playwright/test';

const REVIEW_ID = 'live-local-review-e2e-1';
const serverApiBaseUrl = process.env.OPENSESSION_E2E_SERVER_BASE_URL?.trim();

test.describe('Live Local Review Route', () => {
	// @covers web.live.review.local.render
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
		await expect(page.getByText('feat: review local route live test')).toBeVisible();
		await expect(page.getByRole('heading', { name: 'Live Review Session' })).toBeVisible();
	});
});
