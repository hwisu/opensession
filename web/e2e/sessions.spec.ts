import { test, expect } from '@playwright/test';
import { createSessionFixture, mockSessionApis, type SessionEventSpec } from './helpers';

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

	test('session list repo selector filters by git_repo_name', async ({ page }) => {
		const repoA = createSessionFixture({
			title: `PW Repo A ${crypto.randomUUID().slice(0, 8)}`,
		});
		repoA.summary.git_repo_name = 'org/repo-a';
		const repoB = createSessionFixture({
			title: `PW Repo B ${crypto.randomUUID().slice(0, 8)}`,
		});
		repoB.summary.git_repo_name = 'org/repo-b';

		let requestedRepo: string | null = null;
		await page.route('**/api/sessions**', async (route) => {
			const url = new URL(route.request().url());
			requestedRepo = url.searchParams.get('git_repo_name');
			const selected = requestedRepo;
			const sessions =
				selected === 'org/repo-a'
					? [repoA.summary]
					: selected === 'org/repo-b'
						? [repoB.summary]
						: [repoA.summary, repoB.summary];
			await route.fulfill({
				status: 200,
				contentType: 'application/json',
				body: JSON.stringify({
					sessions,
					total: sessions.length,
					page: 1,
					per_page: 20,
				}),
			});
		});

		await page.goto('/sessions');
		await expect(page.getByText(repoA.title)).toBeVisible({ timeout: 10000 });
		await expect(page.getByText(repoB.title)).toBeVisible({ timeout: 10000 });

		const repoFilterInput = page.locator('#session-repo-filter');
		await repoFilterInput.fill('org/repo-a');
		await repoFilterInput.press('Enter');

		await expect(page.getByText(repoA.title)).toBeVisible({ timeout: 10000 });
		await expect(page.getByText(repoB.title)).toHaveCount(0);
		expect(requestedRepo).toBe('org/repo-a');
	});

	test('session list copy shortcut copies selected session title', async ({ page }) => {
		const fixture = createSessionFixture({
			title: `PW Copy Target ${crypto.randomUUID().slice(0, 8)}`,
		});
		await mockSessionApis(page, fixture);
		await page.addInitScript(() => {
			const clipboardState = { value: '' };
			(window as { __pwClipboard?: { value: string } }).__pwClipboard = clipboardState;
			Object.defineProperty(navigator, 'clipboard', {
				configurable: true,
				value: {
					writeText: async (text: string) => {
						clipboardState.value = text;
					},
					readText: async () => clipboardState.value,
				},
			});
		});
		await page.goto('/sessions');
		await expect(page.getByText(fixture.title)).toBeVisible({ timeout: 10000 });
		await page.locator('body').click();
		await page.keyboard.press('ControlOrMeta+C');
		await expect(page.getByTestId('session-copy-feedback')).toHaveText('Copied');
		const copiedText = await page.evaluate(() => {
			return (window as { __pwClipboard?: { value: string } }).__pwClipboard?.value ?? '';
		});
		expect(copiedText).toBe(fixture.title);
	});

	test('session detail semantic filters support 1-0 numeric shortcuts', async ({ page }) => {
		const fixture = createSessionFixture({
			title: `PW Semantic Filters ${crypto.randomUUID().slice(0, 8)}`,
		});
		const now = new Date().toISOString();
		const rawJsonl = [
			JSON.stringify({
				type: 'header',
				version: 'hail-1.0.0',
				session_id: fixture.id,
				agent: {
					provider: 'openai',
					model: 'gpt-5',
					tool: 'codex',
				},
				context: {
					title: fixture.title,
					description: 'semantic filter regression',
					tags: ['e2e'],
					created_at: now,
					updated_at: now,
					related_session_ids: [],
					attributes: {},
				},
			}),
			JSON.stringify({
				type: 'event',
				event_id: crypto.randomUUID(),
				timestamp: now,
				event_type: { type: 'UserMessage' },
				task_id: 'task-user-1',
				content: { blocks: [{ type: 'Text', text: 'user event row' }] },
				duration_ms: null,
				attributes: {},
			}),
			JSON.stringify({
				type: 'event',
				event_id: crypto.randomUUID(),
				timestamp: now,
				event_type: { type: 'AgentMessage' },
				task_id: null,
				content: { blocks: [{ type: 'Text', text: 'agent event row' }] },
				duration_ms: null,
				attributes: {},
			}),
			JSON.stringify({
				type: 'event',
				event_id: crypto.randomUUID(),
				timestamp: now,
				event_type: { type: 'Thinking' },
				task_id: null,
				content: { blocks: [{ type: 'Text', text: 'thinking event row' }] },
				duration_ms: null,
				attributes: {},
			}),
			JSON.stringify({
				type: 'event',
				event_id: crypto.randomUUID(),
				timestamp: now,
				event_type: { type: 'ToolCall', data: { name: 'exec_command' } },
				task_id: null,
				content: { blocks: [{ type: 'Text', text: 'tool event row' }] },
				duration_ms: null,
				attributes: {},
			}),
			JSON.stringify({
				type: 'event',
				event_id: crypto.randomUUID(),
				timestamp: now,
				event_type: { type: 'FileRead', data: { path: 'README.md' } },
				task_id: null,
				content: { blocks: [{ type: 'Text', text: 'file event row' }] },
				duration_ms: null,
				attributes: {},
			}),
			JSON.stringify({
				type: 'event',
				event_id: crypto.randomUUID(),
				timestamp: now,
				event_type: { type: 'ShellCommand', data: { command: 'ls -la' } },
				task_id: null,
				content: { blocks: [{ type: 'Text', text: 'shell event row' }] },
				duration_ms: null,
				attributes: {},
			}),
			JSON.stringify({
				type: 'event',
				event_id: crypto.randomUUID(),
				timestamp: now,
				event_type: { type: 'TaskStart', data: { title: 'task event row' } },
				task_id: 'task-1',
				content: { blocks: [{ type: 'Text', text: 'task event row' }] },
				duration_ms: null,
				attributes: {},
			}),
			JSON.stringify({
				type: 'event',
				event_id: crypto.randomUUID(),
				timestamp: now,
				event_type: { type: 'WebSearch', data: { query: 'opensession filters' } },
				task_id: null,
				content: { blocks: [{ type: 'Text', text: 'web event row' }] },
				duration_ms: null,
				attributes: {},
			}),
			JSON.stringify({
				type: 'event',
				event_id: crypto.randomUUID(),
				timestamp: now,
				event_type: { type: 'ImageGenerate', data: { prompt: 'other event row' } },
				task_id: null,
				content: { blocks: [{ type: 'Text', text: 'other event row' }] },
				duration_ms: null,
				attributes: {},
			}),
			JSON.stringify({
				type: 'stats',
				event_count: 9,
				message_count: 2,
				tool_call_count: 1,
				task_count: 1,
				duration_seconds: 1,
				total_input_tokens: 0,
				total_output_tokens: 0,
				user_message_count: 1,
				files_changed: 0,
				lines_added: 0,
				lines_removed: 0,
			}),
		].join('\n');

		await mockSessionApis(page, {
			...fixture,
			raw_jsonl: `${rawJsonl}\n`,
		});

		await page.goto(`/session/${fixture.id}`);
		await expect(page.locator('[data-timeline-idx]')).toHaveCount(9);
		await expect(page.locator('[data-filter-key="all"]')).toHaveAttribute('aria-pressed', 'true');

		await page.locator('body').click();
		await page.keyboard.press('2');
		await expect(page.locator('[data-filter-key="all"]')).toHaveAttribute('aria-pressed', 'false');
		await expect(page.locator('[data-filter-key="user"]')).toHaveAttribute('aria-pressed', 'true');
		await expect(page.locator('[data-timeline-idx]')).toHaveCount(1);

		await page.keyboard.press('1');
		await expect(page.locator('[data-filter-key="all"]')).toHaveAttribute('aria-pressed', 'true');
		await expect(page.locator('[data-timeline-idx]')).toHaveCount(9);

		await page.keyboard.press('0');
		await expect(page.locator('[data-filter-key="all"]')).toHaveAttribute('aria-pressed', 'false');
		await expect(page.locator('[data-filter-key="other"]')).toHaveAttribute('aria-pressed', 'true');
		await expect(page.locator('[data-timeline-idx]')).toHaveCount(1);
	});

	test('session detail branchpoints mode focuses on semantic decision nodes', async ({ page }) => {
		const fixture = createSessionFixture({
			title: `PW Branchpoints ${crypto.randomUUID().slice(0, 8)}`,
		});
		const now = new Date().toISOString();
		const rawJsonl = [
			JSON.stringify({
				type: 'header',
				version: 'hail-1.0.0',
				session_id: fixture.id,
				agent: {
					provider: 'openai',
					model: 'gpt-5',
					tool: 'codex',
				},
				context: {
					title: fixture.title,
					description: 'branchpoints regression',
					tags: ['e2e'],
					created_at: now,
					updated_at: now,
					related_session_ids: [],
					attributes: {},
				},
			}),
			JSON.stringify({
				type: 'event',
				event_id: crypto.randomUUID(),
				timestamp: now,
				event_type: { type: 'UserMessage' },
				task_id: 'task-user-1',
				content: { blocks: [{ type: 'Text', text: 'branch user question' }] },
				duration_ms: null,
				attributes: {},
			}),
			JSON.stringify({
				type: 'event',
				event_id: crypto.randomUUID(),
				timestamp: now,
				event_type: { type: 'AgentMessage' },
				task_id: null,
				content: { blocks: [{ type: 'Text', text: 'branch agent answer' }] },
				duration_ms: null,
				attributes: {},
			}),
			JSON.stringify({
				type: 'event',
				event_id: crypto.randomUUID(),
				timestamp: now,
				event_type: { type: 'SystemMessage' },
				task_id: null,
				content: { blocks: [{ type: 'Text', text: 'branch system note' }] },
				duration_ms: null,
				attributes: {},
			}),
			JSON.stringify({
				type: 'event',
				event_id: crypto.randomUUID(),
				timestamp: now,
				event_type: { type: 'TaskStart', data: { title: 'branch task start' } },
				task_id: 'task-1',
				content: { blocks: [{ type: 'Text', text: 'branch task start' }] },
				duration_ms: null,
				attributes: {},
			}),
			JSON.stringify({
				type: 'event',
				event_id: crypto.randomUUID(),
				timestamp: now,
				event_type: { type: 'ToolResult', data: { name: 'exec_command', is_error: true } },
				task_id: null,
				content: { blocks: [{ type: 'Text', text: 'branch tool error' }] },
				duration_ms: null,
				attributes: {},
			}),
			JSON.stringify({
				type: 'event',
				event_id: crypto.randomUUID(),
				timestamp: now,
				event_type: { type: 'FileRead', data: { path: 'README.md' } },
				task_id: null,
				content: { blocks: [{ type: 'Text', text: 'branch file read' }] },
				duration_ms: null,
				attributes: {},
			}),
			JSON.stringify({
				type: 'event',
				event_id: crypto.randomUUID(),
				timestamp: now,
				event_type: { type: 'ShellCommand', data: { command: 'exit 1', exit_code: 1 } },
				task_id: null,
				content: { blocks: [{ type: 'Text', text: 'branch shell error' }] },
				duration_ms: null,
				attributes: {},
			}),
			JSON.stringify({
				type: 'stats',
				event_count: 7,
				message_count: 3,
				tool_call_count: 1,
				task_count: 1,
				duration_seconds: 1,
				total_input_tokens: 0,
				total_output_tokens: 0,
				user_message_count: 1,
				files_changed: 0,
				lines_added: 0,
				lines_removed: 0,
			}),
		].join('\n');

		await mockSessionApis(page, {
			...fixture,
			raw_jsonl: `${rawJsonl}\n`,
		});

		await page.goto(`/session/${fixture.id}`);
		await expect(page.locator('[data-timeline-idx]')).toHaveCount(7);

		await page.getByRole('tab', { name: 'Branchpoints' }).click();
		await expect(page.locator('[data-filter-key="all"]')).toHaveAttribute('aria-pressed', 'true');
		await expect(page.locator('[data-timeline-idx]')).toHaveCount(6);
		await expect(page.getByText('branch file read')).toHaveCount(0);

		await page.locator('body').click();
		await page.keyboard.press('2');
		await expect(page.locator('[data-filter-key="question"]')).toHaveAttribute('aria-pressed', 'true');
		await expect(page.locator('[data-filter-key="all"]')).toHaveAttribute('aria-pressed', 'false');
		await expect(page.locator('[data-timeline-idx]')).toHaveCount(1);
		await expect(page.getByText('branch user question')).toBeVisible({ timeout: 10000 });
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

	test('session flow track drag scrolls timeline smoothly', async ({ page }) => {
		const events: SessionEventSpec[] = Array.from({ length: 90 }, (_, idx) => ({
			type: idx % 2 === 0 ? 'UserMessage' : 'AgentMessage',
			text: `PW Flow Drag Event ${idx + 1}`,
			task_id: idx % 2 === 0 ? `task-${Math.floor(idx / 2) + 1}` : null,
		}));
		const fixture = createSessionFixture({
			title: 'PW Flow Drag Scroll',
			events,
		});
		await mockSessionApis(page, fixture);
		await page.goto(`/session/${fixture.id}`);

		const timeline = page.getByTestId('session-timeline-scroll');
		const flowTrack = page.getByTestId('session-flow-track');
		await expect(flowTrack).toBeVisible({ timeout: 10000 });
		await expect(timeline).toBeVisible({ timeout: 10000 });
		const box = await flowTrack.boundingBox();
		if (!box) throw new Error('flow track bounding box is unavailable');

		await page.mouse.move(box.x + 2, box.y + box.height / 2);
		await page.mouse.down();
		await page.mouse.move(box.x + box.width - 2, box.y + box.height / 2, {
			steps: 16,
		});
		await page.mouse.up();
		await page.waitForTimeout(120);

		await expect(page.getByText('PW Flow Drag Event 90')).toBeVisible({ timeout: 10000 });
	});

	test('session timeline removes keepalive dots and coalesces duplicated thinking rows', async ({
		page,
	}) => {
		const fixture = createSessionFixture({
			title: `PW Timeline Sanitize ${crypto.randomUUID().slice(0, 8)}`,
		});
		const now = new Date().toISOString();
		const rawJsonl = [
			JSON.stringify({
				type: 'header',
				version: 'hail-1.0.0',
				session_id: fixture.id,
				agent: {
					provider: 'openai',
					model: 'gpt-5',
					tool: 'codex',
				},
				context: {
					title: fixture.title,
					description: 'timeline sanitize regression',
					tags: ['e2e'],
					created_at: now,
					updated_at: now,
					related_session_ids: [],
					attributes: {},
				},
			}),
			JSON.stringify({
				type: 'event',
				event_id: crypto.randomUUID(),
				timestamp: now,
				event_type: { type: 'UserMessage' },
				task_id: null,
				content: { blocks: [{ type: 'Text', text: 'run timeline sanity check' }] },
				duration_ms: null,
				attributes: {},
			}),
			JSON.stringify({
				type: 'event',
				event_id: crypto.randomUUID(),
				timestamp: now,
				event_type: { type: 'Thinking' },
				task_id: null,
				content: { blocks: [{ type: 'Text', text: '**Inspecting store and URL files**' }] },
				duration_ms: null,
				attributes: {},
			}),
			JSON.stringify({
				type: 'event',
				event_id: crypto.randomUUID(),
				timestamp: now,
				event_type: { type: 'Thinking' },
				task_id: null,
				content: { blocks: [{ type: 'Text', text: '**Inspecting store and URL files**' }] },
				duration_ms: null,
				attributes: {},
			}),
			JSON.stringify({
				type: 'event',
				event_id: crypto.randomUUID(),
				timestamp: now,
				event_type: {
					type: 'ToolResult',
					data: { name: 'exec_command', is_error: false, call_id: 'call-dot' },
				},
				task_id: null,
				content: { blocks: [{ type: 'Text', text: '.' }] },
				duration_ms: null,
				attributes: { 'semantic.call_id': 'call-dot' },
			}),
			JSON.stringify({
				type: 'event',
				event_id: crypto.randomUUID(),
				timestamp: now,
				event_type: {
					type: 'ToolResult',
					data: { name: 'exec_command', is_error: false, call_id: 'call-chunk' },
				},
				task_id: null,
				content: { blocks: [{ type: 'Text', text: 'Chunk ID: keep123' }] },
				duration_ms: null,
				attributes: { 'semantic.call_id': 'call-chunk' },
			}),
			JSON.stringify({
				type: 'stats',
				event_count: 6,
				message_count: 1,
				tool_call_count: 0,
				task_count: 0,
				duration_seconds: 1,
				total_input_tokens: 0,
				total_output_tokens: 0,
				user_message_count: 1,
				files_changed: 0,
				lines_added: 0,
				lines_removed: 0,
			}),
		].join('\n');

		await mockSessionApis(page, {
			...fixture,
			raw_jsonl: `${rawJsonl}\n`,
		});

		await page.goto(`/session/${fixture.id}`);
		await expect(page.locator('[data-timeline-idx]')).toHaveCount(3);
		await expect(page.getByTestId('session-flow-bar')).toContainText('5 events');
		await expect(page.locator('[data-timeline-idx] [data-event-type="Thinking"]')).toHaveCount(1);
		await expect(page.getByText('Chunk ID: keep123')).toBeVisible({ timeout: 10000 });
		await expect(page.getByText(/^\.$/)).toHaveCount(0);
	});

	test('session detail shows agent info', async ({ page }) => {
		const fixture = createSessionFixture();
		await mockSessionApis(page, fixture);
		await page.goto(`/session/${fixture.id}`);
		await expect(page.getByText('Claude Code').first()).toBeVisible({ timeout: 10000 });
	});

	test('session detail sidebar renders metadata glyphs', async ({ page }) => {
		const fixture = createSessionFixture({
			title: `PW Sidebar Glyph ${crypto.randomUUID().slice(0, 8)}`,
		});
		await mockSessionApis(page, fixture);
		await page.goto(`/session/${fixture.id}`);
		const sidebar = page.getByTestId('session-detail-sidebar');
		await expect(sidebar).toBeVisible({ timeout: 10000 });
		const glyphCount = await sidebar.locator('svg').count();
		expect(glyphCount).toBeGreaterThanOrEqual(9);
		await expect(sidebar).toContainText('Model:');
		await expect(sidebar).toContainText('Tool:');
		await expect(sidebar).toContainText('Provider:');
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
