import { test, expect, type APIRequestContext } from '@playwright/test';
import { getAdmin, injectAuth, createTeam } from './helpers';

async function ensureTeamsApiAvailable(request: APIRequestContext, accessToken: string) {
	const probe = await request.get('/api/teams', {
		headers: { Authorization: `Bearer ${accessToken}` },
	});
	test.skip(probe.status() === 404, 'teams UI/API is disabled for this profile');
}

test.describe('Teams', () => {
	test('teams page loads for authenticated user', async ({ page, request }) => {
		const admin = await getAdmin(request);
		await ensureTeamsApiAvailable(request, admin.access_token);
		await injectAuth(page, admin);
		await page.goto('/teams');
		await expect(page.locator('main')).toBeVisible();
	});

	test('created team appears in teams list', async ({ page, request }) => {
		const admin = await getAdmin(request);
		await ensureTeamsApiAvailable(request, admin.access_token);
		const teamName = `pw-team-${crypto.randomUUID().slice(0, 6)}`;
		await createTeam(request, admin.access_token, teamName);

		await injectAuth(page, admin);
		await page.goto('/teams');

		await expect(page.getByText(teamName)).toBeVisible({ timeout: 10000 });
	});

	test('navigate to team detail page', async ({ page, request }) => {
		const admin = await getAdmin(request);
		await ensureTeamsApiAvailable(request, admin.access_token);
		const teamName = `pw-detail-${crypto.randomUUID().slice(0, 6)}`;
		const teamId = await createTeam(request, admin.access_token, teamName);

		await injectAuth(page, admin);
		await page.goto(`/teams/${teamId}`);

		await expect(page.getByText(teamName)).toBeVisible({ timeout: 10000 });
	});
});
