import { test, expect } from '@playwright/test';

test.describe('Docs', () => {
	test('docs markdown is rendered as structured HTML (not raw #/``` text)', async ({ page }) => {
		await page.goto('/docs');
		await expect(page.locator('main h1').first()).toBeVisible();
		await expect(page.locator('main h1').first()).toContainText('Documentation');
		await expect(page.locator('main article').getByText('# Documentation', { exact: true })).toHaveCount(0);
		await expect(page.locator('main article').getByText('```bash')).toHaveCount(0);
	});

	test('config section shows canonical path and watcher/git-storage example', async ({ page }) => {
		await page.goto('/docs');
		await expect(page.locator('main').getByText('~/.config/opensession/opensession.toml')).toBeVisible();
		await expect(page.locator('main').getByText('custom_paths = [')).toBeVisible();
		await expect(page.locator('main').getByText('[git_storage]')).toBeVisible();
		await expect(page.locator('main').getByText('method = "native"')).toBeVisible();
	});

	test('docs snippet no longer shows legacy watcher toggles and old select command', async ({ page }) => {
		await page.goto('/docs');
		await expect(page.locator('main').getByText('claude_code = true')).toHaveCount(0);
		await expect(page.locator('main').getByText('opencode = true')).toHaveCount(0);
		await expect(page.locator('main').getByText('cursor = false')).toHaveCount(0);
		await expect(page.locator('main').getByText('daemon select --agent')).toHaveCount(0);
		await expect(page.locator('main').getByText('daemon select --repo .')).toHaveCount(0);
		await expect(page.locator('main').getByText('opensession daemon start --repo .')).toBeVisible();
	});
});
