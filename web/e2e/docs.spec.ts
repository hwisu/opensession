import { expect, test } from '@playwright/test';

const chapterHeadings = [
	'Product Overview',
	'Core Capabilities',
	'Runtime Profiles',
	'Web Routes',
	'CLI Workflows',
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

	test('docs uses structured sections and table-first summaries', async ({ page }) => {
		await page.goto('/docs');
		const docsContent = page.getByTestId('docs-content');
		await expect(docsContent.getByRole('heading', { level: 2, name: 'Product Overview' })).toBeVisible();
		await expect(docsContent.getByRole('heading', { level: 2, name: 'Runtime Profiles' })).toBeVisible();
		await expect(docsContent.getByRole('heading', { level: 2, name: 'API Summary' })).toBeVisible();
		await expect(docsContent.getByRole('columnheader', { name: 'Capability' }).first()).toBeVisible();
		await expect(docsContent.getByText('Server (Axum)')).toBeVisible();
		await expect(docsContent.getByRole('cell', { name: '/sessions' }).first()).toBeVisible();
		await expect(
			docsContent.getByRole('cell', { name: '/gh/{owner}/{repo}/{ref}/{path...}' }).first(),
		).toBeVisible();
	});

	test('docs include usage examples and avoid legacy route docs', async ({ page }) => {
		await page.goto('/docs');
		const docsContent = page.getByTestId('docs-content');

		await expect(docsContent.getByText('opensession publish upload ./session.jsonl')).toBeVisible();
		await expect(docsContent.getByText('opensession publish upload ./session.jsonl --git')).toBeVisible();
		await expect(docsContent.getByText('Product goal')).toBeVisible();
		await expect(docsContent.getByText('use high-signal public sessions')).toBeVisible();
		await expect(docsContent.getByText('wrangler dev --ip 127.0.0.1 --port 8788 --persist-to .wrangler/state')).toBeVisible();

		await expect(docsContent.getByText('/teams', { exact: false })).toHaveCount(0);
		await expect(docsContent.getByText('/invitations', { exact: false })).toHaveCount(0);
		await expect(docsContent.getByRole('heading', { level: 2, name: 'Teams' })).toHaveCount(0);
	});

	test('shows chapter navigation table of contents', async ({ page }) => {
		await page.setViewportSize({ width: 1400, height: 900 });
		await page.goto('/docs');
		const toc = page.getByTestId('docs-toc');
		await expect(toc).toBeVisible();
		for (const heading of chapterHeadings) {
			await expect(toc.getByRole('link', { name: heading })).toBeVisible();
		}
	});
});
