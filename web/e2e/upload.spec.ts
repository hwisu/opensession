import { expect, test } from '@playwright/test';

test.describe('Removed Upload Route', () => {
	test('/upload is removed and resolves as not found', async ({ page }) => {
		await page.goto('/upload');
		await expect(page.getByRole('heading', { name: '404' })).toBeVisible();
		await expect(page.getByText('Not Found')).toBeVisible();
		await expect(page.getByRole('heading', { name: 'Upload Session' })).toHaveCount(0);
	});
});
