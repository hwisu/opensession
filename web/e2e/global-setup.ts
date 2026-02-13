import { request } from '@playwright/test';

const BASE_URL = process.env.BASE_URL || 'http://localhost:3000';

/**
 * Global setup: register the admin user before any tests run.
 * On self-hosted Docker, the first registered user becomes admin.
 */
async function globalSetup() {
	const ctx = await request.newContext({ baseURL: BASE_URL });

	// Register admin as the very first user
	const resp = await ctx.post('/api/auth/register', {
		data: {
			email: 'pw-admin@e2e.local',
			password: 'testpass99',
			nickname: 'pw-admin',
		},
	});

	if (resp.ok()) {
		console.log('Global setup: admin user registered (first user = admin)');
	} else {
		const body = await resp.text();
		// "email already registered" is fine — means admin exists from a previous run
		if (body.includes('already')) {
			console.log('Global setup: admin user already exists');
		} else {
			console.warn(`Global setup: register returned ${resp.status()}: ${body}`);
		}
	}

	await ctx.dispose();
}

export default globalSetup;
