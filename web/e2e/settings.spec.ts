import { expect, test } from '@playwright/test';

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000';

test.describe('Settings', () => {
	test('settings page requires auth when no token exists', async ({ page }) => {
		await page.goto('/settings');
		await expect(page.locator('[data-testid="settings-require-auth"]')).toBeVisible();
		await expect(page.locator('[data-testid="settings-require-auth"]')).toContainText(
			'Sign in is required',
		);
	});

	test('can issue personal api key from settings page', async ({ page }) => {
		const secure = BASE_URL.startsWith('https://');
		const domain = new URL(BASE_URL).hostname;
		const now = Math.floor(Date.now() / 1000);
		await page.context().addCookies([
			{
				name: 'opensession_csrf_token',
				value: 'settings-csrf-token',
				domain,
				path: '/',
				httpOnly: false,
				secure,
				sameSite: 'Lax',
				expires: now + 3600,
			},
		]);

		await page.route('**/api/capabilities', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					auth_enabled: true,
					parse_preview_enabled: true,
					register_targets: ['local', 'git'],
					share_modes: ['web', 'git', 'json'],
				}),
			});
		});

		await page.route('**/api/auth/verify', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({ user_id: 'u-settings', nickname: 'settings-user' }),
			});
		});

		await page.route('**/api/auth/me', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					user_id: 'u-settings',
					nickname: 'settings-user',
					created_at: '2026-02-21T00:00:00.000Z',
					email: 'settings@test.local',
					avatar_url: null,
					oauth_providers: [
						{ provider: 'github', provider_username: 'settings-user', display_name: 'GitHub' },
					],
				}),
			});
		});

		const credentials: Array<{
			id: string;
			label: string;
			host: string;
			path_prefix: string;
			header_name: string;
			created_at: string;
			updated_at: string;
			last_used_at: string | null;
		}> = [
			{
				id: 'cred-1',
				label: 'GitLab Internal',
				host: 'gitlab.internal.example.com',
				path_prefix: 'group/subgroup',
				header_name: 'Authorization',
				created_at: '2026-02-21T00:00:00.000Z',
				updated_at: '2026-02-21T00:00:00.000Z',
				last_used_at: null,
			},
		];
		await page.route('**/api/auth/git-credentials', async (route) => {
			if (route.request().method() === 'GET') {
				await route.fulfill({
					status: 200,
					contentType: 'application/json',
					body: JSON.stringify({ credentials }),
				});
				return;
			}
			if (route.request().method() === 'POST') {
				const body = route.request().postDataJSON() as {
					label?: string;
					host?: string;
					path_prefix?: string | null;
					header_name?: string;
				};
				const created = {
					id: `cred-${credentials.length + 1}`,
					label: body.label ?? 'unknown',
					host: body.host ?? 'unknown',
					path_prefix: body.path_prefix ?? '',
					header_name: body.header_name ?? 'Authorization',
					created_at: '2026-02-22T00:00:00.000Z',
					updated_at: '2026-02-22T00:00:00.000Z',
					last_used_at: null,
				};
				credentials.push(created);
				await route.fulfill({
					status: 201,
					contentType: 'application/json',
					body: JSON.stringify(created),
				});
				return;
			}
			await route.fallback();
		});
		await page.route('**/api/auth/git-credentials/*', async (route) => {
			if (route.request().method() !== 'DELETE') {
				await route.fallback();
				return;
			}
			const id = route.request().url().split('/').pop() ?? '';
			const decoded = decodeURIComponent(id);
			const index = credentials.findIndex((credential) => credential.id === decoded);
			if (index >= 0) {
				credentials.splice(index, 1);
				await route.fulfill({
					status: 200,
					contentType: 'application/json',
					body: JSON.stringify({ ok: true }),
				});
				return;
			}
			await route.fulfill({
				status: 404,
				contentType: 'application/json',
				body: JSON.stringify({ code: 'not_found', message: 'credential not found' }),
			});
		});

		await page.route('**/api/auth/api-keys/issue', async (route) => {
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({ api_key: 'osk_settings_e2e_test_key' }),
			});
		});

		await page.goto('/settings');
		await expect(page.locator('[data-testid="settings-page"]')).toBeVisible();
		await expect(page.locator('[data-testid="settings-page"]')).toContainText('settings-user');
		await expect(page.locator('[data-testid="settings-page"]')).toContainText('settings@test.local');
		await expect(page.locator('[data-testid="git-credential-settings"]')).toContainText(
			'GitLab Internal',
		);

		await page.locator('[data-testid="issue-api-key-button"]').click();
		await expect(page.locator('[data-testid="issued-api-key"]')).toContainText(
			'osk_settings_e2e_test_key',
		);
		await expect(page.locator('[data-testid="copy-api-key"]')).toBeVisible();

		await page.locator('[data-testid="git-credential-label"]').fill('Generic Git');
		await page.locator('[data-testid="git-credential-host"]').fill('code.example.com');
		await page.locator('[data-testid="git-credential-path-prefix"]').fill('team/repo');
		await page.locator('[data-testid="git-credential-header-name"]').fill('Authorization');
		await page.locator('[data-testid="git-credential-header-value"]').fill('Bearer abc123');
		await page.locator('[data-testid="git-credential-save"]').click();
		await expect(page.locator('[data-testid="git-credential-settings"]')).toContainText('Generic Git');

		await page.locator('[data-testid="git-credential-delete-cred-1"]').click();
		await expect(page.locator('[data-testid="git-credential-settings"]')).not.toContainText(
			'GitLab Internal',
		);
	});
});
