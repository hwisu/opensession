import { expect, test } from '@playwright/test';

const chapterHeadings = [
	'Product Overview',
	'Web Experience',
	'CLI Workflows',
	'TUI Workflows',
	'Handoff Storage Model',
	'Web Routes',
	'API Summary',
	'Self-Hosting and Verification',
	'Troubleshooting',
];

test.describe('Docs', () => {
	test('renders chapter-based docs structure from markdown', async ({ page }) => {
		await page.goto('/docs');
		const docsPage = page.getByTestId('docs-page');
		await expect(docsPage.getByRole('heading', { level: 1, name: 'Documentation' })).toBeVisible();
		await expect(docsPage.getByText('# Documentation', { exact: true })).toHaveCount(0);

		for (const heading of chapterHeadings) {
			await expect(page.getByTestId('docs-content').getByRole('heading', { level: 2, name: heading })).toBeVisible();
		}
	});

	test('docs describe /git preview route and remove upload page docs', async ({ page }) => {
		await page.goto('/docs');
		const docsContent = page.getByTestId('docs-content');
		await expect(docsContent.getByRole('heading', { level: 2, name: 'Web Routes' })).toBeVisible();
		await expect(docsContent.getByRole('cell', { name: '/git' }).first()).toBeVisible();
		await expect(
			docsContent.getByRole('cell', { name: '/gh/{owner}/{repo}/{ref}/{path...}' }).first(),
		).toBeVisible();
		await expect(docsContent.getByRole('cell', { name: '/upload' })).toHaveCount(0);
	});

	test('docs include concise git-sharing definition and current commands', async ({ page }) => {
		await page.goto('/docs');
		const docsContent = page.getByTestId('docs-content');

		await expect(
			docsContent.getByText('Git-based sharing means storing session artifacts on the opensession/sessions branch'),
		).toBeVisible();
		await expect(docsContent.getByText('opensession publish upload ./session.jsonl')).toBeVisible();
		await expect(docsContent.getByText('opensession publish upload ./session.jsonl --git')).toBeVisible();
		await expect(docsContent.getByText('wrangler dev --ip 127.0.0.1 --port 8788 --persist-to .wrangler/state')).toBeVisible();
	});

	test('docs omit legacy capability-matrix language', async ({ page }) => {
		await page.goto('/docs');
		const docsContent = page.getByTestId('docs-content');
		await expect(docsContent.getByRole('heading', { level: 2, name: 'Core Goals' })).toHaveCount(0);
		await expect(docsContent.getByRole('heading', { level: 2, name: 'Runtime Profiles' })).toHaveCount(0);
		await expect(docsContent.getByText('Runtime Capability Matrix')).toHaveCount(0);
		await expect(docsContent.getByText('auth_enabled')).toHaveCount(0);
		await expect(docsContent.getByText('upload_enabled')).toHaveCount(0);
	});

	test('shows sticky chapter navigation table of contents', async ({ page }) => {
		await page.setViewportSize({ width: 1400, height: 900 });
		await page.goto('/docs');
		const toc = page.getByTestId('docs-toc');
		await expect(toc).toBeVisible();
		const stickyStyle = await toc.evaluate((el) => getComputedStyle(el).position);
		expect(stickyStyle).toBe('sticky');
		const stickyTop = await toc.evaluate((el) => getComputedStyle(el).top);
		expect(stickyTop).not.toBe('auto');
		for (const heading of chapterHeadings) {
			await expect(toc.getByRole('link', { name: heading })).toBeVisible();
		}

		const before = await toc.boundingBox();
		expect(before).not.toBeNull();
		await page.locator('main').evaluate((el) => {
			el.scrollTop = 1200;
		});
		await page.waitForTimeout(150);
		const afterFirst = await toc.boundingBox();
		expect(afterFirst).not.toBeNull();
		await page.locator('main').evaluate((el) => {
			el.scrollTop = 2200;
		});
		await page.waitForTimeout(150);
		const afterSecond = await toc.boundingBox();
		expect(afterSecond).not.toBeNull();
		expect(Math.abs((afterSecond?.y ?? 0) - (afterFirst?.y ?? 0))).toBeLessThan(6);

		await page.evaluate(() => window.scrollTo(0, 1200));
		await page.waitForTimeout(150);
		const afterWindow = await toc.boundingBox();
		expect(afterWindow).not.toBeNull();
		expect(Math.abs((afterWindow?.y ?? 0) - (afterSecond?.y ?? 0))).toBeLessThan(6);
	});

	test('keeps docs in dark surface styling', async ({ page }) => {
		await page.goto('/docs');
		const docsPage = page.getByTestId('docs-page');
		const bg = await docsPage.evaluate((el) => getComputedStyle(el).color);
		expect(bg).not.toBe('rgb(19, 19, 19)');
	});

	test('toc navigation keeps chapter linking stable', async ({ page }) => {
		await page.setViewportSize({ width: 1400, height: 900 });
		await page.goto('/docs');
		const toc = page.getByTestId('docs-toc');
		const targetLink = toc.getByRole('link', { name: 'TUI Workflows' });
		await targetLink.click();
		await expect(page).toHaveURL(/#tui-workflows$/);
		await expect(page.getByRole('heading', { level: 2, name: 'TUI Workflows' })).toBeVisible();
		await expect(toc).toBeVisible();
	});
});
