import { expect, test } from '@playwright/test';

test.describe('DX Lab', () => {
	test('conformance dashboard shows five parser rows', async ({ page }) => {
		await page.goto('/dx');
		await expect(page.getByText('Parser Conformance Dashboard')).toBeVisible();
		await expect(page.locator('[data-testid="dx-conformance-table"] tbody tr')).toHaveCount(5);
		await expect(page.locator('[data-testid="dx-coverage-score"]')).toContainText('%');
	});

	test('parser playground reports parse errors and recovers with sample', async ({ page }) => {
		await page.goto('/dx');

		const input = page.locator('[data-testid="dx-raw-input"]');
		await input.fill('this is not valid json');
		await page.getByRole('button', { name: 'Run parse' }).click();
		await expect(page.locator('[data-testid="dx-parse-error"]')).toBeVisible();

		await page.getByRole('button', { name: 'Load sample' }).click();
		await expect(page.locator('[data-testid="dx-parse-error"]')).toHaveCount(0);
		await expect(page.locator('[data-testid="dx-event-count"]')).toHaveText('4');
	});
});
