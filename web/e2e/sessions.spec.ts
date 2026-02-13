import { test, expect } from '@playwright/test';
import { getAdmin, injectAuth, uploadSession, createTeam } from './helpers';

test.describe('Sessions', () => {
	test('upload and view session in list', async ({ page, request }) => {
		const admin = await getAdmin(request);
		const teamId = await createTeam(request, admin.access_token);
		const title = `PW List ${crypto.randomUUID().slice(0, 8)}`;
		const sessionId = await uploadSession(request, admin.access_token, {
			title,
			teamId,
		});

		await injectAuth(page, admin);
		await page.goto('/');

		// Session should appear in the list
		await expect(page.getByText(title)).toBeVisible({ timeout: 10000 });
	});

	test('navigate to session detail', async ({ page, request }) => {
		const admin = await getAdmin(request);
		const teamId = await createTeam(request, admin.access_token);
		const sessionId = await uploadSession(request, admin.access_token, {
			title: 'PW Detail Test',
			teamId,
		});

		await injectAuth(page, admin);
		await page.goto(`/session/${sessionId}`);

		// Should show session detail with events
		await expect(page.getByText('Hello, write a test')).toBeVisible({ timeout: 10000 });
	});

	test('session detail shows agent info', async ({ page, request }) => {
		const admin = await getAdmin(request);
		const teamId = await createTeam(request, admin.access_token);
		const sessionId = await uploadSession(request, admin.access_token, { teamId });

		await injectAuth(page, admin);
		await page.goto(`/session/${sessionId}`);

		// Should show the tool/model info somewhere (UI renders display name "Claude Code")
		await expect(page.getByText('Claude Code').first()).toBeVisible({ timeout: 10000 });
	});

	test('empty session list shows appropriate state', async ({ page, request }) => {
		const admin = await getAdmin(request);
		await injectAuth(page, admin);
		await page.goto('/');

		// Should be on the session list page (no landing hero)
		await expect(page.locator('h1').filter({ hasText: 'AI sessions are' })).not.toBeVisible();
	});
});
