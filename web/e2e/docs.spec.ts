import { test, expect } from '@playwright/test';

test.describe('Docs', () => {
	test('daemon config snippet uses custom_paths and daemon capture policy', async ({ page }) => {
		await page.goto('/docs');
		await expect(
			page.locator('main').getByText('opensession.toml', { exact: true }).first(),
		).toBeVisible();
		await expect(page.locator('main').getByText('auto_publish = false')).toBeVisible();
		await expect(page.locator('main').getByText('Daemon Capture')).toBeVisible();
		await expect(page.locator('main').getByText('session_end | realtime | manual')).toBeVisible();
		await expect(page.locator('main').getByText('custom_paths = [')).toBeVisible();
	});

	test('docs snippet no longer shows legacy watcher toggles', async ({ page }) => {
		await page.goto('/docs');
		await expect(page.locator('main').getByText('claude_code = true')).toHaveCount(0);
		await expect(page.locator('main').getByText('opencode = true')).toHaveCount(0);
		await expect(page.locator('main').getByText('cursor = false')).toHaveCount(0);
		await expect(page.locator('main').getByText('daemon select --agent')).toHaveCount(0);
		await expect(page.locator('main').getByText('daemon select --repo .')).toBeVisible();
	});
});
