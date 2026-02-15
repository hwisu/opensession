import { test, expect } from '@playwright/test';

test.describe('Landing Page (unauthenticated)', () => {
	test('shows landing page with hero text', async ({ page }) => {
		await page.goto('/');
		await expect(page.locator('h1')).toContainText('AI sessions are');
		await expect(page.locator('h1')).toContainText("Don't let them");
	});

	test('shows supported tools section', async ({ page }) => {
		await page.goto('/');
		await expect(page.getByText('Works with your tools.')).toBeVisible();
		await expect(page.getByText('Claude Code')).toBeVisible();
		await expect(page.getByText('Cursor')).toBeVisible();
	});

	test('shows HAIL spec section', async ({ page }) => {
		await page.goto('/');
		await expect(page.getByText('HAIL — Human-AI Interaction Log')).toBeVisible();
	});

	test('shows feature cards', async ({ page }) => {
		await page.goto('/');
		await expect(page.getByText('Open Format', { exact: true })).toBeVisible();
		await expect(page.getByText('Self-Hostable', { exact: true })).toBeVisible();
	});

	test('nav bar shows opensession.io branding', async ({ page }) => {
		await page.goto('/');
		await expect(page.locator('nav').getByText('opensession')).toBeVisible();
	});

	test('nav bar shows Login link when unauthenticated', async ({ page }) => {
		await page.goto('/');
		await expect(page.locator('nav').getByText('Login')).toBeVisible();
	});

	test('terminal animation renders lines', async ({ page }) => {
		await page.goto('/');
		// Wait for the typewriter animation to show at least the first line
		await expect(
			page.getByText('$ opensession upload ./session.jsonl'),
		).toBeVisible({ timeout: 10000 });
	});
});
