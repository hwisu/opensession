import { test, expect } from '@playwright/test';
import { createSessionFixture, mockSessionApis } from './helpers';

test.describe('Sessions', () => {
	test('shows session in list', async ({ page }) => {
		const fixture = createSessionFixture({
			title: `PW List ${crypto.randomUUID().slice(0, 8)}`,
		});
		await mockSessionApis(page, fixture);
		await page.goto('/sessions');
		await expect(page.getByText(fixture.title)).toBeVisible({ timeout: 10000 });
	});

	test('session list search filters correctly', async ({ page }) => {
		const fixture = createSessionFixture({
			title: `PW Shortcut Search ${crypto.randomUUID().slice(0, 8)}`,
		});
		await mockSessionApis(page, fixture);
		await page.goto('/sessions');
		const listSearchInput = page.locator('#session-search');
		await listSearchInput.fill(fixture.title);
		await page.keyboard.press('Enter');
		await expect(page.getByText(fixture.title)).toBeVisible({ timeout: 10000 });
	});

	test('navigate to session detail', async ({ page }) => {
		const fixture = createSessionFixture({
			title: 'PW Detail Test',
			events: [
				{ type: 'UserMessage', text: 'Hello, write a test', task_id: crypto.randomUUID() },
				{ type: 'AgentMessage', text: 'Sure, here is a test.', task_id: null },
			],
		});
		await mockSessionApis(page, fixture);
		await page.goto(`/session/${fixture.id}`);
		await expect(page.getByText('Hello, write a test')).toBeVisible({ timeout: 10000 });
		await expect(page.locator('[data-testid="session-flow-bar"]')).toBeVisible({ timeout: 10000 });
	});

	test('session detail shows agent info', async ({ page }) => {
		const fixture = createSessionFixture();
		await mockSessionApis(page, fixture);
		await page.goto(`/session/${fixture.id}`);
		await expect(page.getByText('Claude Code').first()).toBeVisible({ timeout: 10000 });
	});

	test('session detail light mode uses theme-driven surfaces', async ({ page }) => {
		const fixture = createSessionFixture({
			title: `PW Light Theme ${crypto.randomUUID().slice(0, 8)}`,
		});
		await mockSessionApis(page, fixture);
		await page.addInitScript(() => {
			localStorage.setItem('theme', 'light');
			document.documentElement.classList.add('light');
		});
		await page.goto(`/session/${fixture.id}`);

		const hero = page.getByTestId('session-detail-hero');
		const heroBackground = await hero.evaluate((el) => getComputedStyle(el).backgroundImage);
		expect(heroBackground).not.toContain('24, 33, 50');
		expect(heroBackground).not.toContain('15, 20, 31');

		const sidebar = page.getByTestId('session-detail-sidebar');
		await expect(sidebar).toBeVisible();
		const sidebarBackground = await sidebar.evaluate((el) => getComputedStyle(el).backgroundImage);
		expect(sidebarBackground).not.toContain('24, 33, 50');
		expect(sidebarBackground).not.toContain('14, 19, 29');

		const titleRgb = await hero.getByRole('heading', { level: 1 }).evaluate((el) => {
			const match = getComputedStyle(el).color.match(/\d+/g);
			if (!match || match.length < 3) return { r: 255, g: 255, b: 255 };
			return {
				r: Number(match[0]),
				g: Number(match[1]),
				b: Number(match[2]),
			};
		});
		expect(titleRgb.r).toBeLessThan(90);
		expect(titleRgb.g).toBeLessThan(90);
		expect(titleRgb.b).toBeLessThan(90);
	});

	test('session detail in-session search shortcuts work', async ({ page }) => {
		const fixture = createSessionFixture({
			title: 'PW In-Session Search',
			events: [
				{
					type: 'UserMessage',
					text: 'Find the session needle here',
					task_id: crypto.randomUUID(),
				},
				{
					type: 'AgentMessage',
					text: 'Second event for baseline visibility',
					task_id: null,
				},
			],
		});
		await mockSessionApis(page, fixture);
		await page.goto(`/session/${fixture.id}`);

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
	}) => {
		const fixture = createSessionFixture({
			title: 'PW Markdown/Code Render',
			events: [
				{
					type: 'UserMessage',
					text: '# Plan\n\nRead the [guide](https://example.com) first.\n\n- Parse logs\n- Improve rendering',
					task_id: crypto.randomUUID(),
				},
				{
					type: 'AgentMessage',
					text: '```ts\nconst answer = 42;\nconsole.log(answer);\n```',
					task_id: null,
				},
			],
		});
		await mockSessionApis(page, fixture);
		await page.goto(`/session/${fixture.id}`);

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
		await expect(
			page.locator('.code-header span').filter({ hasText: 'ts' }).first(),
		).toBeVisible({
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

	test('empty session list shows appropriate state', async ({ page }) => {
		const empty = {
			sessions: [],
			total: 0,
			page: 1,
			per_page: 50,
		};
		await page.route('**/api/sessions', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify(empty),
			});
		});
		await page.route('**/api/sessions?*', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify(empty),
			});
		});
		await page.goto('/sessions');
		await expect(page.locator('#session-search')).toBeVisible();
	});
});
