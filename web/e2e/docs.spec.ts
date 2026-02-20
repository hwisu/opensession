import { expect, test } from '@playwright/test';

const chapterHeadings = [
	'Product Map',
	'Capture Sessions',
	'Explore Sessions',
	'GitHub Share Preview',
	'Auth & Access',
	'Runtime Profiles',
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

	test('each chapter keeps What/How/Example/Limits template', async ({ page }) => {
		await page.goto('/docs');
		const docsContent = page.getByTestId('docs-content');
		const minChapters = chapterHeadings.length;
		await expect(docsContent.getByRole('heading', { level: 2, name: 'Product Map' })).toBeVisible();

		const whatCount = await docsContent.getByRole('heading', { level: 3, name: 'What it does' }).count();
		const howCount = await docsContent.getByRole('heading', { level: 3, name: 'How to use' }).count();
		const exampleCount = await docsContent.getByRole('heading', { level: 3, name: 'Example' }).count();
		const limitsCount = await docsContent.getByRole('heading', { level: 3, name: 'Limits' }).count();

		expect(whatCount).toBeGreaterThanOrEqual(minChapters);
		expect(howCount).toBeGreaterThanOrEqual(minChapters);
		expect(exampleCount).toBeGreaterThanOrEqual(minChapters);
		expect(limitsCount).toBeGreaterThanOrEqual(minChapters);
	});

	test('docs include usage examples and avoid legacy route docs', async ({ page }) => {
		await page.goto('/docs');
		const docsContent = page.getByTestId('docs-content');

		await expect(docsContent.getByText('opensession publish upload ./session.jsonl')).toBeVisible();
		await expect(docsContent.getByText('/gh/hwisu/opensession/main/sessions/demo.hail.jsonl')).toBeVisible();
		await expect(
			docsContent.getByText('wrangler dev --ip 127.0.0.1 --port 8788 --persist-to .wrangler/state'),
		).toBeVisible();

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
