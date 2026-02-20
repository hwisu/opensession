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
		await expect(page.locator('main h1').first()).toBeVisible();
		await expect(page.locator('main h1').first()).toContainText('Documentation');
		await expect(page.locator('main article').getByText('# Documentation', { exact: true })).toHaveCount(0);

		for (const heading of chapterHeadings) {
			await expect(
				page.locator('main article').getByRole('heading', { level: 2, name: heading }),
			).toBeVisible();
		}
	});

	test('each chapter keeps What/How/Example/Limits template', async ({ page }) => {
		await page.goto('/docs');
		const article = page.locator('main article');
		const minChapters = chapterHeadings.length;
		await expect(article.getByRole('heading', { level: 1, name: 'Documentation' })).toBeVisible();

		const whatCount = await article.getByRole('heading', { level: 3, name: 'What it does' }).count();
		const howCount = await article.getByRole('heading', { level: 3, name: 'How to use' }).count();
		const exampleCount = await article.getByRole('heading', { level: 3, name: 'Example' }).count();
		const limitsCount = await article.getByRole('heading', { level: 3, name: 'Limits' }).count();

		expect(whatCount).toBeGreaterThanOrEqual(minChapters);
		expect(howCount).toBeGreaterThanOrEqual(minChapters);
		expect(exampleCount).toBeGreaterThanOrEqual(minChapters);
		expect(limitsCount).toBeGreaterThanOrEqual(minChapters);
	});

	test('docs include usage examples and avoid legacy route docs', async ({ page }) => {
		await page.goto('/docs');
		const article = page.locator('main article');

		await expect(article.getByText('opensession publish upload ./session.jsonl')).toBeVisible();
		await expect(article.getByText('/gh/hwisu/opensession/main/sessions/demo.hail.jsonl')).toBeVisible();
		await expect(
			article.getByText('wrangler dev --ip 127.0.0.1 --port 8788 --persist-to .wrangler/state'),
		).toBeVisible();

		await expect(article.getByText('/teams', { exact: false })).toHaveCount(0);
		await expect(article.getByText('/invitations', { exact: false })).toHaveCount(0);
		await expect(article.getByRole('heading', { level: 2, name: 'Teams' })).toHaveCount(0);
	});
});
