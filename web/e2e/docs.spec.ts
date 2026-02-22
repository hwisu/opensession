import { expect, test } from '@playwright/test';

const chapterHeadings = [
	'Getting Started',
	'Share via Git',
	'Inspect Timeline',
	'Handoff',
	'Optional UI',
	'Concepts',
];

test.describe('Docs', () => {
	test('renders chapter-based docs structure from markdown', async ({ page }) => {
		await page.goto('/docs');
		const docsPage = page.getByTestId('docs-page');
		await expect(docsPage.getByRole('heading', { level: 1, name: 'Documentation' })).toBeVisible();
		await expect(docsPage.getByText('# Documentation', { exact: true })).toHaveCount(0);

		for (const heading of chapterHeadings) {
			await expect(
				page.getByTestId('docs-content').getByRole('heading', { level: 2, name: heading }),
			).toBeVisible();
		}
	});

	test('docs include v1 commands and routes', async ({ page }) => {
		await page.goto('/docs');
		const docsContent = page.getByTestId('docs-content');

		await expect(
			docsContent
				.locator('code')
				.filter({ hasText: 'opensession register ./session.hail.jsonl' })
				.first(),
		).toBeVisible();
		await expect(
			docsContent
				.locator('code')
				.filter({ hasText: 'opensession share os://src/local/<sha256> --git --remote origin' })
				.first(),
		).toBeVisible();
		await expect(
			docsContent
				.locator('code')
				.filter({
					hasText:
						'opensession handoff artifacts get os://artifact/<sha256> --format canonical --encode jsonl',
				})
				.first(),
		).toBeVisible();
		await expect(docsContent.getByText('POST /api/parse/preview')).toBeVisible();
		await expect(docsContent.getByText('/src/gh/').first()).toBeVisible();
		await expect(docsContent.getByText('/src/git/').first()).toBeVisible();
	});

	test('docs omit legacy publish/ingest wording', async ({ page }) => {
		await page.goto('/docs');
		const docsContent = page.getByTestId('docs-content');
		await expect(docsContent.getByText('opensession publish upload')).toHaveCount(0);
		await expect(docsContent.getByText('opensession session handoff')).toHaveCount(0);
		await expect(docsContent.getByText('/api/ingest/preview')).toHaveCount(0);
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
	});

	test('toc navigation keeps chapter linking stable', async ({ page }) => {
		await page.setViewportSize({ width: 1400, height: 900 });
		await page.goto('/docs');
		const toc = page.getByTestId('docs-toc');
		const targetLink = toc.getByRole('link', { name: 'Optional UI' });
		await targetLink.click();
		await expect(page).toHaveURL(/#optional-ui$/);
		await expect(page.getByRole('heading', { level: 2, name: 'Optional UI' })).toBeVisible();
		await expect(toc).toBeVisible();
	});
});
