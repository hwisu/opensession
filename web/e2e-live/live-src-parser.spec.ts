import { expect, test } from '@playwright/test';

function base64UrlEncode(value: string): string {
	return Buffer.from(value, 'utf8')
		.toString('base64')
		.replace(/\+/g, '-')
		.replace(/\//g, '_')
		.replace(/=+$/g, '');
}

const serverApiBaseUrl = process.env.OPENSESSION_E2E_SERVER_BASE_URL?.trim();

test.describe('Live Source Route', () => {
	// @covers web.live.src.git.parser_selection_retry
	test('parser selection route retries with explicit parser hint', async ({ page }) => {
		test.skip(
			!serverApiBaseUrl,
			'Set OPENSESSION_E2E_SERVER_BASE_URL to run src parser live route against real API',
		);

		const parserHints: Array<string | null> = [];
		page.on('request', (request) => {
			if (request.method() !== 'POST') return;
			if (request.url() !== `${serverApiBaseUrl}/api/parse/preview`) return;
			const body = request.postData();
			if (!body) return;
			try {
				const parsed = JSON.parse(body) as { parser_hint?: string | null };
				const hint = typeof parsed.parser_hint === 'string' ? parsed.parser_hint : null;
				parserHints.push(hint);
			} catch {
				// Ignore malformed payloads; request body shape is asserted by parser hint checks.
			}
		});

		await page.goto('/');
		await page.evaluate((apiUrl) => {
			localStorage.setItem('opensession_api_url', apiUrl);
		}, serverApiBaseUrl!);

		const remote = base64UrlEncode('https://github.com/hwisu/opensession');
		await page.goto(`/src/git/${remote}/ref/main/path/crates/parsers/tests/fixtures/cursor/composer_data.json`);

		await expect(page.getByRole('heading', { name: 'Parser selection required' })).toBeVisible();

		const parserPanel = page.locator('section', {
			has: page.getByRole('heading', { name: 'Parser selection required' }),
		});
		const firstCandidate = parserPanel.getByRole('button').first();
		await expect(firstCandidate).toBeVisible();
		await firstCandidate.click();
		await expect(page).toHaveURL(/(?:\?|&)parser_hint=[a-z0-9-]+/i);

		await expect.poll(() => parserHints.length).toBeGreaterThanOrEqual(2);
		expect(parserHints.some((hint) => hint == null)).toBeTruthy();
		expect(parserHints.some((hint) => typeof hint === 'string' && hint.length > 0)).toBeTruthy();
	});
});
