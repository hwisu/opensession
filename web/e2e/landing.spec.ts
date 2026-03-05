import { test, expect } from '@playwright/test';
import { createSessionFixture, mockSessionApis } from './helpers';

test.describe('Landing (unauthenticated)', () => {
	test('shows landing sections and nav for guests, without inline session list', async ({ page }) => {
		await page.goto('/');
		await expect(page.locator('nav').getByText('Sessions')).toBeVisible();
		await expect(
			page.locator('h1').filter({ hasText: 'Version Control for AI Work.' }),
		).toBeVisible();
		await expect(page.getByTestId('landing-hero-copy')).toContainText(
			'OpenSession turns AI sessions into structured, replayable artifacts',
		);
		await expect(page.locator('#session-search')).toHaveCount(0);
		await expect(page.locator('nav').getByText('Login')).toBeVisible();
		await expect(page.locator('nav').getByText('Register')).toHaveCount(0);
	});

	test('desktop runtime uses compact landing sections to avoid extra scrolling', async ({ page }) => {
		await page.addInitScript(() => {
			(window as Window & { __TAURI_INTERNALS__?: Record<string, never> }).__TAURI_INTERNALS__ = {};
		});
		await page.goto('/');
		await expect(page.locator('h1').filter({ hasText: 'Version Control for AI Work.' })).toBeVisible();
		await expect(page.locator('[data-contract-section="goal-map"]')).toHaveCount(0);
		await expect(page.locator('[data-contract-section="operating-loop"]')).toHaveCount(0);
	});

	test('desktop runtime without auth does not show login navigation', async ({ page }) => {
		await page.addInitScript(() => {
			(window as Window & { __TAURI_INTERNALS__?: Record<string, never> }).__TAURI_INTERNALS__ = {};
		});
		await page.goto('/');
		await expect(page.locator('nav').getByText('Login')).toHaveCount(0);
		await expect(page.locator('nav').getByText('Settings')).toBeVisible();
	});

	test('desktop runtime can open settings route without auth API', async ({ page }) => {
		await page.addInitScript(() => {
			(window as Window & { __TAURI_INTERNALS__?: Record<string, never> }).__TAURI_INTERNALS__ = {};
		});
		await page.goto('/settings');
		await expect(page.locator('[data-testid="settings-page"]')).toBeVisible();
		await expect(page.locator('[data-testid="settings-auth-disabled"]')).toHaveCount(0);
		await expect(page.locator('[data-testid="runtime-summary-settings"]')).toBeVisible();
	});

	test('desktop runtime loads docs via local IPC without /docs HTTP dependency', async ({ page }) => {
		await page.addInitScript(() => {
			(window as Window & { __TAURI_INTERNALS__?: Record<string, never> }).__TAURI_INTERNALS__ = {};
			(window as Window & { __TAURI__?: unknown }).__TAURI__ = {
				core: {
					invoke: async (cmd: string) => {
						if (cmd === 'desktop_get_contract_version') return { version: 'desktop-ipc-v6' };
						if (cmd === 'desktop_get_capabilities') {
							return {
								auth_enabled: false,
								parse_preview_enabled: false,
								register_targets: [],
								share_modes: [],
							};
						}
						if (cmd === 'desktop_get_auth_providers') {
							return { email_password: false, oauth: [] };
						}
						if (cmd === 'desktop_get_docs_markdown') {
							return ['# Documentation', '', '## Local Docs', '', 'Desktop docs from IPC.'].join('\n');
						}
						throw new Error(`unexpected command: ${cmd}`);
					},
				},
			};
		});
		await page.route('**/docs?format=markdown', async (route) => {
			await route.fulfill({ status: 500, body: 'should not be called in desktop docs mode' });
		});
		await page.goto('/docs');
		await expect(page.getByRole('heading', { level: 2, name: 'Local Docs' })).toBeVisible({
			timeout: 10000,
		});
		await expect(page.locator('main')).toContainText('Desktop docs from IPC.');
	});

	test('landing renders goal sections without capability matrix language', async ({ page }) => {
		await page.goto('/');
		await expect(page.locator('[data-contract-section="goal-map"]')).toBeVisible();
		await expect(page.locator('[data-contract-section="operating-loop"]')).toBeVisible();
		await expect(page.locator('[data-contract-section="capability-matrix"]')).toHaveCount(0);
		await expect(page.getByText('Runtime Capability Matrix')).toHaveCount(0);
	});

	test('landing copy is English-only in hero and section copy', async ({ page }) => {
		await page.goto('/');
		const hero = await page.getByTestId('landing-hero-copy').textContent();
		const goals = await page.locator('[data-contract-section="goal-map"]').textContent();
		const loop = await page.locator('[data-contract-section="operating-loop"]').textContent();

		expect(hero ?? '').not.toMatch(/[가-힣]/);
		expect(goals ?? '').not.toMatch(/[가-힣]/);
		expect(loop ?? '').not.toMatch(/[가-힣]/);
	});

	test('landing quick action opens docs page', async ({ page }) => {
		await page.goto('/');
		await page.getByRole('button', { name: 'Open Docs' }).click();
		await expect(page).toHaveURL(/\/docs$/);
		await expect(page.locator('main h1').first()).toContainText('Documentation');
	});

	test('landing beginner quick start opens getting-started docs anchor', async ({ page }) => {
		await page.route('**/docs?format=markdown', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'text/markdown; charset=utf-8',
				body: ['# Documentation', '', '## Getting Started', '', 'Begin here'].join('\n'),
			});
		});
		await page.goto('/');
		await page.getByRole('button', { name: 'Beginner Quick Start' }).click();
		await expect(page).toHaveURL(/\/docs#getting-started$/);
		await expect(page.getByRole('heading', { level: 2, name: 'Getting Started' })).toBeVisible();
	});

	test('login page is accessible to guests', async ({ page }) => {
		await page.route('**/api/capabilities', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					auth_enabled: true,
					parse_preview_enabled: false,
					register_targets: ['local', 'git'],
					share_modes: ['web', 'git', 'json'],
				}),
			});
		});
		await page.route('**/api/auth/providers', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({ email_password: true, oauth: [] }),
			});
		});

		await page.goto('/login');
		await expect(page).toHaveURL(/\/login$/);
		await expect(page.locator('#login-email')).toBeVisible();
		await expect(page.locator('#login-nickname')).toBeVisible();
	});

	test('register path redirects to login', async ({ page }) => {
		await page.goto('/register');
		await expect(page).toHaveURL(/\/login$/);
	});

	test('renders public session without authentication', async ({ page }) => {
		const fixture = createSessionFixture({
			title: `PW Public Feed ${crypto.randomUUID().slice(0, 8)}`,
		});
		await mockSessionApis(page, fixture);

		await page.goto('/sessions');
		await expect(page.locator('#session-search')).toBeVisible();
		await expect(page.locator('main').getByText(fixture.title)).toBeVisible({ timeout: 10000 });

		await page.goto(`/session/${fixture.id}`);
		await expect(page).toHaveURL(new RegExp(`/session/${fixture.id}$`));
		await expect(page.locator('main').getByText(fixture.title)).toBeVisible({ timeout: 10000 });
	});
});
