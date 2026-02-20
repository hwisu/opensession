import { test, expect } from '@playwright/test';
import { getAdmin, getCapabilities, injectAuth, uploadSession } from './helpers';

test.describe('Sessions', () => {
	test('upload and view session in list', async ({ page, request }) => {
		const capabilities = await getCapabilities(request);
		test.skip(!capabilities.auth_enabled, 'Auth API is disabled');
		test.skip(!capabilities.upload_enabled, 'Upload API is disabled');

		const admin = await getAdmin(request);
		const title = `PW List ${crypto.randomUUID().slice(0, 8)}`;
		await uploadSession(request, admin.access_token, {
			title,
		});

		await injectAuth(page, admin);
		await page.goto('/');

		// Session should appear in the list
		await expect(page.getByText(title)).toBeVisible({ timeout: 10000 });
	});

	test('session list search filters correctly', async ({ page, request }) => {
		const capabilities = await getCapabilities(request);
		test.skip(!capabilities.auth_enabled, 'Auth API is disabled');
		test.skip(!capabilities.upload_enabled, 'Upload API is disabled');

		const admin = await getAdmin(request);
		const title = `PW Shortcut Search ${crypto.randomUUID().slice(0, 8)}`;
		await uploadSession(request, admin.access_token, { title });

		await injectAuth(page, admin);
		await page.goto('/');
		const listSearchInput = page.locator('#session-search');
		await listSearchInput.fill(title);
		await page.keyboard.press('Enter');
		await expect(page.getByText(title)).toBeVisible({ timeout: 10000 });
	});

	test('navigate to session detail', async ({ page, request }) => {
		const capabilities = await getCapabilities(request);
		test.skip(!capabilities.auth_enabled, 'Auth API is disabled');
		test.skip(!capabilities.upload_enabled, 'Upload API is disabled');

		const admin = await getAdmin(request);
		const sessionId = await uploadSession(request, admin.access_token, {
			title: 'PW Detail Test',
		});

		await injectAuth(page, admin);
		await page.goto(`/session/${sessionId}`);

		// Should show session detail with events
		await expect(page.getByText('Hello, write a test')).toBeVisible({ timeout: 10000 });
		await expect(page.locator('[data-testid="session-flow-bar"]')).toBeVisible({ timeout: 10000 });
	});

	test('session detail shows agent info', async ({ page, request }) => {
		const capabilities = await getCapabilities(request);
		test.skip(!capabilities.auth_enabled, 'Auth API is disabled');
		test.skip(!capabilities.upload_enabled, 'Upload API is disabled');

		const admin = await getAdmin(request);
		const sessionId = await uploadSession(request, admin.access_token);

		await injectAuth(page, admin);
		await page.goto(`/session/${sessionId}`);

		// Should show the tool/model info somewhere (UI renders display name "Claude Code")
		await expect(page.getByText('Claude Code').first()).toBeVisible({ timeout: 10000 });
	});

	test('session detail in-session search shortcuts work', async ({ page, request }) => {
		const capabilities = await getCapabilities(request);
		test.skip(!capabilities.auth_enabled, 'Auth API is disabled');
		test.skip(!capabilities.upload_enabled, 'Upload API is disabled');

		const admin = await getAdmin(request);
		const now = Date.now();
		const sessionId = await uploadSession(request, admin.access_token, {
			title: 'PW In-Session Search',
			events: [
				{
					event_id: crypto.randomUUID(),
					timestamp: new Date(now).toISOString(),
					event_type: { type: 'UserMessage' },
					task_id: crypto.randomUUID(),
					content: { blocks: [{ type: 'Text', text: 'Find the session needle here' }] },
					duration_ms: null,
					attributes: {},
				},
				{
					event_id: crypto.randomUUID(),
					timestamp: new Date(now + 1000).toISOString(),
					event_type: { type: 'AgentMessage' },
					task_id: null,
					content: { blocks: [{ type: 'Text', text: 'Second event for baseline visibility' }] },
					duration_ms: null,
					attributes: {},
				},
			],
		});

		await injectAuth(page, admin);
		await page.goto(`/session/${sessionId}`);

		const detailSearchInput = page.locator('#session-event-search');
		await detailSearchInput.fill('needle');
		await page.keyboard.press('Enter');

		await expect(page.getByText('Find the session needle here')).toBeVisible({ timeout: 10000 });
		await expect(page.getByText('1 matches')).toBeVisible({ timeout: 10000 });
		await expect(page.locator('[data-timeline-idx]')).toHaveCount(1);

		await page.keyboard.press('Escape');
		await expect(detailSearchInput).toHaveValue('');
		await expect(page.locator('[data-timeline-idx]')).toHaveCount(2);
	});

	test('session detail renders markdown and standalone fenced code for messages', async ({
		page,
		request,
	}) => {
		const capabilities = await getCapabilities(request);
		test.skip(!capabilities.auth_enabled, 'Auth API is disabled');
		test.skip(!capabilities.upload_enabled, 'Upload API is disabled');

		const admin = await getAdmin(request);
		const now = Date.now();
		const sessionId = await uploadSession(request, admin.access_token, {
			title: 'PW Markdown/Code Render',
			events: [
				{
					event_id: crypto.randomUUID(),
					timestamp: new Date(now).toISOString(),
					event_type: { type: 'UserMessage' },
					task_id: crypto.randomUUID(),
					content: {
						blocks: [
							{
								type: 'Text',
								text: '# Plan\n\nRead the [guide](https://example.com) first.\n\n- Parse logs\n- Improve rendering',
							},
						],
					},
					duration_ms: null,
					attributes: {},
				},
				{
					event_id: crypto.randomUUID(),
					timestamp: new Date(now + 1000).toISOString(),
					event_type: { type: 'AgentMessage' },
					task_id: null,
					content: {
						blocks: [
							{
								type: 'Text',
								text: '```ts\nconst answer = 42;\nconsole.log(answer);\n```',
							},
						],
					},
					duration_ms: null,
					attributes: {},
				},
			],
		});

		await injectAuth(page, admin);
		await page.goto(`/session/${sessionId}`);

		await expect(page.locator('.md-content .md-h1').filter({ hasText: 'Plan' })).toBeVisible({
			timeout: 10000,
		});
		await expect(page.locator('.md-content li').filter({ hasText: 'Parse logs' })).toBeVisible({
			timeout: 10000,
		});
		await expect(page.locator('.md-content .md-link').filter({ hasText: 'guide' })).toBeVisible({
			timeout: 10000,
		});
		await expect(
			page.locator('.code-with-lines code').filter({ hasText: 'const answer = 42;' }).first(),
		).toBeVisible({ timeout: 10000 });
		await expect(page.locator('.code-header span').filter({ hasText: 'ts' }).first()).toBeVisible({
			timeout: 10000,
		});

		const proseStyles = await page.locator('.md-content .md-p').first().evaluate((el) => {
			const p = getComputedStyle(el);
			const linkEl = el.querySelector('.md-link') as HTMLElement | null;
			const link = linkEl ? getComputedStyle(linkEl) : null;
			return {
				fontSize: Number.parseFloat(p.fontSize),
				lineHeight: Number.parseFloat(p.lineHeight),
				paragraphColor: p.color,
				linkColor: link?.color ?? '',
			};
		});
		expect(proseStyles.lineHeight / proseStyles.fontSize).toBeGreaterThan(1.6);
		expect(proseStyles.linkColor).not.toBe(proseStyles.paragraphColor);
	});

	test('empty session list shows appropriate state', async ({ page, request }) => {
		const capabilities = await getCapabilities(request);
		test.skip(!capabilities.auth_enabled, 'Auth API is disabled');

		const admin = await getAdmin(request);
		await injectAuth(page, admin);
		await page.goto('/');

		// Should be on the session list page.
		await expect(page.locator('#session-search')).toBeVisible();
	});

	test('upload drop zone keeps active drag state through nested drag events', async ({
		page,
		request,
	}) => {
		const capabilities = await getCapabilities(request);
		test.skip(!capabilities.auth_enabled, 'Auth API is disabled');
		test.skip(!capabilities.upload_enabled, 'Upload API is disabled');

		const admin = await getAdmin(request);
		await injectAuth(page, admin);
		await page.goto('/upload');

		const dropZone = page.getByRole('button', { name: /Drag and drop a session file here/i }).first();
		await expect(dropZone).toBeVisible({ timeout: 10000 });

		const dataTransfer = await page.evaluateHandle(() => new DataTransfer());
		await dropZone.dispatchEvent('dragenter', { dataTransfer });
		await expect(dropZone).toHaveClass(/border-accent/);

		const browseLabel = dropZone.getByText('Browse files');
		await browseLabel.dispatchEvent('dragenter', { dataTransfer });
		await browseLabel.dispatchEvent('dragleave', { dataTransfer });
		await expect(dropZone).toHaveClass(/border-accent/);

		await dropZone.dispatchEvent('dragleave', { dataTransfer });
		await expect(dropZone).not.toHaveClass(/border-accent/);
	});
});
