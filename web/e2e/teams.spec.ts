import { test, expect } from '@playwright/test';
import { getAdmin, injectAuth, createTeam } from './helpers';

test.describe('Teams', () => {
	test('teams page loads for authenticated user', async ({ page, request }) => {
		const admin = await getAdmin(request);
		await injectAuth(page, admin);
		await page.goto('/teams');
		await expect(page.locator('main')).toBeVisible();
	});

	test('created team appears in teams list', async ({ page, request }) => {
		const admin = await getAdmin(request);
		const teamName = `pw-team-${crypto.randomUUID().slice(0, 6)}`;
		await createTeam(request, admin.access_token, teamName);

		await injectAuth(page, admin);
		await page.goto('/teams');

		await expect(page.getByText(teamName)).toBeVisible({ timeout: 10000 });
	});

	test('navigate to team detail page', async ({ page, request }) => {
		const admin = await getAdmin(request);
		const teamName = `pw-detail-${crypto.randomUUID().slice(0, 6)}`;
		const teamId = await createTeam(request, admin.access_token, teamName);

		await injectAuth(page, admin);
		await page.goto(`/teams/${teamId}`);

		await expect(page.getByText(teamName)).toBeVisible({ timeout: 10000 });
	});
});
