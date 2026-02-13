<script lang="ts">
const {
	onNavigate = (_path: string) => {},
}: {
	onNavigate?: (path: string) => void;
} = $props();

const sections = [
	{ id: 'getting-started', flag: '--init', title: 'Getting Started' },
	{ id: 'sessions', flag: '--upload', title: 'Sessions' },
	{ id: 'teams', flag: '--team', title: 'Teams' },
	{ id: 'invitations', flag: '--invite', title: 'Invitations' },
	{ id: 'team-stats', flag: '--stats', title: 'Team Stats' },
	{ id: 'cli', flag: '--cli', title: 'CLI Reference' },
	{ id: 'self-hosting', flag: '--host', title: 'Self-Hosting' },
	{ id: 'hail-format', flag: '--spec', title: 'HAIL Format' },
];

function scrollTo(id: string) {
	document.getElementById(id)?.scrollIntoView({ behavior: 'smooth' });
}
</script>

<div class="docs-root">
	<div class="mx-auto max-w-4xl px-6 py-10">
	<!-- Header -->
	<div class="mb-10">
		<div class="mb-3 text-xs uppercase tracking-[0.2em] text-text-muted">
			$ opensession docs
		</div>
		<h1 class="mb-3 text-2xl font-bold text-text-primary md:text-3xl">
			Documentation
		</h1>
		<p class="max-w-2xl text-base leading-relaxed text-text-secondary">
			Everything you need to know about using OpenSession — uploading sessions,
			managing teams, and self-hosting your own instance.
		</p>
	</div>

	<!-- Table of Contents -->
	<nav class="mb-12 border border-border p-5">
		<div class="mb-3 text-xs font-bold text-text-muted uppercase tracking-wider">Contents</div>
		<div class="grid gap-1 sm:grid-cols-2">
			{#each sections as sec}
				<button
					onclick={() => scrollTo(sec.id)}
					class="flex items-center gap-2 px-2 py-1.5 text-left text-sm text-text-secondary transition-colors hover:bg-bg-hover hover:text-accent"
				>
					<span class="text-accent">{sec.flag}</span>
					<span>{sec.title}</span>
				</button>
			{/each}
		</div>
	</nav>

	<!-- Getting Started -->
	<section id="getting-started" class="docs-section mb-12">
		<div class="mb-1 text-xs text-accent">--init</div>
		<h2 class="mb-4 text-lg font-bold text-text-primary">Getting Started</h2>

		<div class="space-y-4 text-sm leading-relaxed text-text-secondary">
			<h3 class="text-sm font-bold text-text-primary">Create an Account</h3>
			<p>
				Sign up at <button onclick={() => onNavigate('/register')} class="text-accent hover:underline">/register</button> with
				a username and password, or use OAuth (GitHub, Google) if available on your instance.
			</p>

			<h3 class="text-sm font-bold text-text-primary">Sign In</h3>
			<p>
				Log in at <button onclick={() => onNavigate('/login')} class="text-accent hover:underline">/login</button>.
				Password and OAuth methods are both supported. After signing in, you'll land on your session list.
			</p>

			<h3 class="text-sm font-bold text-text-primary">Get Your API Key</h3>
			<p>
				Navigate to <button onclick={() => onNavigate('/settings')} class="text-accent hover:underline">/settings</button> to
				find your API key. You'll need this for uploading sessions via the CLI.
			</p>
		</div>
	</section>

	<!-- Sessions -->
	<section id="sessions" class="docs-section mb-12">
		<div class="mb-1 text-xs text-accent">--upload</div>
		<h2 class="mb-4 text-lg font-bold text-text-primary">Sessions</h2>

		<div class="space-y-4 text-sm leading-relaxed text-text-secondary">
			<h3 class="text-sm font-bold text-text-primary">Uploading Sessions</h3>
			<p>There are two ways to upload sessions:</p>

			<div class="border border-border bg-bg-secondary p-4">
				<div class="mb-2 text-xs uppercase tracking-wider text-text-muted">CLI Upload</div>
				<code class="block text-xs text-accent">$ opensession upload ./session.jsonl</code>
				<code class="mt-1 block text-xs text-accent">$ opensession upload-all ~/.claude/projects/</code>
			</div>

			<div class="border border-border bg-bg-secondary p-4">
				<div class="mb-2 text-xs uppercase tracking-wider text-text-muted">Web Upload</div>
				<p class="text-xs">
					Drag and drop <code class="text-accent">.jsonl</code> files onto the
					<button onclick={() => onNavigate('/upload')} class="text-accent hover:underline">/upload</button> page,
					or click to select files. You can optionally assign a team before uploading.
				</p>
			</div>

			<h3 class="text-sm font-bold text-text-primary">Viewing Sessions</h3>
			<p>
				Your session list at <button onclick={() => onNavigate('/')} class="text-accent hover:underline">/</button> shows
				all uploaded sessions. Each card displays the session tool, model, timestamp, token count, and a preview of the conversation.
			</p>

			<h3 class="text-sm font-bold text-text-primary">Timeline View</h3>
			<p>
				Click into any session to see the full timeline — every message, tool call, file edit,
				and error in chronological order. Use the sidebar to jump between events or filter by type.
			</p>

			<h3 class="text-sm font-bold text-text-primary">Search &amp; Filter</h3>
			<p>
				Use the search bar (<kbd class="border border-border bg-bg-secondary px-1.5 py-0.5 text-xs">/</kbd> to focus)
				to search across all sessions by content. Filter by tool, model, or team using the filter controls.
			</p>
		</div>
	</section>

	<!-- Teams -->
	<section id="teams" class="docs-section mb-12">
		<div class="mb-1 text-xs text-accent">--team</div>
		<h2 class="mb-4 text-lg font-bold text-text-primary">Teams</h2>

		<div class="space-y-4 text-sm leading-relaxed text-text-secondary">
			<h3 class="text-sm font-bold text-text-primary">Creating a Team</h3>
			<p>
				Go to <button onclick={() => onNavigate('/teams')} class="text-accent hover:underline">/teams</button> and
				click <strong>Create Team</strong>. Give your team a name — you'll be the admin automatically.
			</p>

			<h3 class="text-sm font-bold text-text-primary">Inviting Members</h3>
			<p>
				From the team detail page, invite members by their email address.
				If they already have an account (including OAuth accounts), the invitation links to their existing account.
				Otherwise, they'll be prompted to register when accepting.
			</p>

			<h3 class="text-sm font-bold text-text-primary">Roles</h3>
			<div class="grid gap-px border border-border bg-border sm:grid-cols-2">
				<div class="bg-bg-primary p-4">
					<div class="mb-1 text-sm font-bold text-accent">admin</div>
					<p class="text-xs">Can invite/remove members, change roles, delete the team, and view all team sessions and stats.</p>
				</div>
				<div class="bg-bg-primary p-4">
					<div class="mb-1 text-sm font-bold text-accent">member</div>
					<p class="text-xs">Can upload sessions to the team and view all team sessions and stats.</p>
				</div>
			</div>

			<h3 class="text-sm font-bold text-text-primary">Managing Members</h3>
			<p>
				Admins can remove members or change their role from the team detail page.
				Removing a member does not delete their previously uploaded sessions.
			</p>
		</div>
	</section>

	<!-- Invitations -->
	<section id="invitations" class="docs-section mb-12">
		<div class="mb-1 text-xs text-accent">--invite</div>
		<h2 class="mb-4 text-lg font-bold text-text-primary">Invitations</h2>

		<div class="space-y-4 text-sm leading-relaxed text-text-secondary">
			<p>
				When you're invited to a team, a badge appears in the top navigation bar.
				Navigate to <button onclick={() => onNavigate('/invitations')} class="text-accent hover:underline">/invitations</button> to
				see all pending invitations.
			</p>

			<h3 class="text-sm font-bold text-text-primary">Accepting</h3>
			<p>
				Click <strong>Accept</strong> to join the team. You'll immediately gain access to all team sessions
				and can start uploading sessions to the team.
			</p>

			<h3 class="text-sm font-bold text-text-primary">Declining</h3>
			<p>
				Click <strong>Decline</strong> to reject the invitation. The team admin can send a new invitation later if needed.
			</p>
		</div>
	</section>

	<!-- Team Stats -->
	<section id="team-stats" class="docs-section mb-12">
		<div class="mb-1 text-xs text-accent">--stats</div>
		<h2 class="mb-4 text-lg font-bold text-text-primary">Team Stats</h2>

		<div class="space-y-4 text-sm leading-relaxed text-text-secondary">
			<p>
				Each team has a stats view showing usage across the team. Access it from the team detail page.
			</p>

			<h3 class="text-sm font-bold text-text-primary">By User</h3>
			<p>
				See how many sessions, tokens, and tool calls each team member has contributed.
			</p>

			<h3 class="text-sm font-bold text-text-primary">By Tool</h3>
			<p>
				Break down usage by AI tool — see which tools your team uses most (Claude Code, Cursor, Codex, etc.).
			</p>

			<h3 class="text-sm font-bold text-text-primary">Time Ranges</h3>
			<p>
				Filter stats by time range to see usage trends over the past week, month, or custom date ranges.
			</p>
		</div>
	</section>

	<!-- CLI -->
	<section id="cli" class="docs-section mb-12">
		<div class="mb-1 text-xs text-accent">--cli</div>
		<h2 class="mb-4 text-lg font-bold text-text-primary">CLI Reference</h2>

		<div class="space-y-4 text-sm leading-relaxed text-text-secondary">
			<h3 class="text-sm font-bold text-text-primary">Installation</h3>
			<div class="border border-border bg-bg-secondary p-4">
				<code class="block text-xs text-accent">$ cargo install opensession</code>
			</div>

			<h3 class="text-sm font-bold text-text-primary">Configuration</h3>
			<p>
				Set your API endpoint and key:
			</p>
			<div class="border border-border bg-bg-secondary p-4">
				<code class="block text-xs text-accent">$ opensession config set api-url https://opensession.io</code>
				<code class="mt-1 block text-xs text-accent">$ opensession config set api-key YOUR_API_KEY</code>
			</div>

			<h3 class="text-sm font-bold text-text-primary">Commands</h3>
			<div class="space-y-3">
				<div class="border border-border p-4">
					<code class="text-xs font-bold text-accent">opensession upload &lt;file&gt;</code>
					<p class="mt-1 text-xs">Upload a single HAIL session file. Supports <code class="text-accent">.jsonl</code> files.</p>
				</div>
				<div class="border border-border p-4">
					<code class="text-xs font-bold text-accent">opensession upload-all &lt;directory&gt;</code>
					<p class="mt-1 text-xs">Recursively scan a directory and upload all HAIL session files found. Skips already-uploaded sessions.</p>
				</div>
				<div class="border border-border p-4">
					<code class="text-xs font-bold text-accent">opensession config set &lt;key&gt; &lt;value&gt;</code>
					<p class="mt-1 text-xs">Set a configuration value. Keys: <code class="text-accent">api-url</code>, <code class="text-accent">api-key</code>.</p>
				</div>
			</div>
		</div>
	</section>

	<!-- Self-Hosting -->
	<section id="self-hosting" class="docs-section mb-12">
		<div class="mb-1 text-xs text-accent">--host</div>
		<h2 class="mb-4 text-lg font-bold text-text-primary">Self-Hosting</h2>

		<div class="space-y-4 text-sm leading-relaxed text-text-secondary">
			<h3 class="text-sm font-bold text-text-primary">Quick Start</h3>
			<p>Run OpenSession with a single command:</p>
			<div class="border border-border bg-bg-secondary p-4">
				<code class="block text-xs text-accent">$ docker run -p 3000:3000 ghcr.io/hwisu/opensession</code>
			</div>
			<p>
				This starts the server on port 3000 with an embedded SQLite database.
				Data persists inside the container — mount a volume for durability.
			</p>

			<h3 class="text-sm font-bold text-text-primary">Docker Compose</h3>
			<p>For production use with persistent storage:</p>
			<div class="border border-border bg-bg-secondary p-4">
				<pre class="text-xs text-accent leading-relaxed">services:
  opensession:
    image: ghcr.io/hwisu/opensession
    ports:
      - "3000:3000"
    volumes:
      - opensession-data:/data
    environment:
      - DATABASE_URL=/data/opensession.db

volumes:
  opensession-data:</pre>
			</div>

			<h3 class="text-sm font-bold text-text-primary">Point the CLI to Your Instance</h3>
			<div class="border border-border bg-bg-secondary p-4">
				<code class="block text-xs text-accent">$ opensession config set api-url http://localhost:3000</code>
			</div>
		</div>
	</section>

	<!-- HAIL Format -->
	<section id="hail-format" class="docs-section mb-12">
		<div class="mb-1 text-xs text-accent">--spec</div>
		<h2 class="mb-4 text-lg font-bold text-text-primary">HAIL Format</h2>

		<div class="space-y-4 text-sm leading-relaxed text-text-secondary">
			<p>
				<strong>HAIL</strong> (Human-AI Interaction Log) is an open JSONL format for recording AI coding sessions.
				Each line is a self-contained JSON object representing one event.
			</p>

			<h3 class="text-sm font-bold text-text-primary">Structure</h3>
			<p>Every HAIL file starts with a metadata line, followed by events:</p>
			<div class="border border-border bg-bg-secondary p-4">
				<pre class="text-xs leading-relaxed"><span class="text-text-muted">// Line 1: Session metadata</span>
<span class="text-accent">{`{"v":"hail/0.1","tool":"claude-code","model":"opus-4","ts":"..."}`}</span>

<span class="text-text-muted">// Subsequent lines: Events</span>
<span class="text-role-human">{`{"role":"human","content":"Fix the auth bug"}`}</span>
<span class="text-role-agent">{`{"role":"agent","content":"I'll update...","tool_calls":[...]}`}</span>
<span class="text-role-tool">{`{"type":"file_edit","path":"src/auth.rs","diff":"..."}`}</span></pre>
			</div>

			<h3 class="text-sm font-bold text-text-primary">Event Types</h3>
			<div class="grid gap-px border border-border bg-border sm:grid-cols-2">
				<div class="bg-bg-primary p-3">
					<span class="text-xs font-bold text-role-human">human</span>
					<p class="mt-0.5 text-xs">User messages and prompts</p>
				</div>
				<div class="bg-bg-primary p-3">
					<span class="text-xs font-bold text-role-agent">agent</span>
					<p class="mt-0.5 text-xs">AI responses and reasoning</p>
				</div>
				<div class="bg-bg-primary p-3">
					<span class="text-xs font-bold text-role-tool">tool_call</span>
					<p class="mt-0.5 text-xs">Tool invocations and file edits</p>
				</div>
				<div class="bg-bg-primary p-3">
					<span class="text-xs font-bold text-error">error</span>
					<p class="mt-0.5 text-xs">Errors and failures</p>
				</div>
			</div>

			<h3 class="text-sm font-bold text-text-primary">Why JSONL?</h3>
			<p>
				Line-oriented JSON is streamable, appendable, and works with standard tools like
				<code class="text-accent">jq</code>, <code class="text-accent">grep</code>, and
				<code class="text-accent">wc -l</code>.
				No special parser needed — any language with JSON support can read HAIL files.
			</p>
		</div>
	</section>

	<!-- Footer CTA -->
	<section class="border-t border-border pt-10 text-center">
		<p class="mb-4 text-base text-text-secondary">
			Ready to get started?
		</p>
		<div class="flex items-center justify-center gap-3">
			<button
				onclick={() => onNavigate('/register')}
				class="bg-accent px-6 py-2.5 text-sm font-bold text-white transition-all hover:shadow-[0_0_20px_rgba(59,130,246,0.3)]"
			>
				Create Account
			</button>
			<a
				href="https://github.com/hwisu/opensession-core"
				target="_blank"
				rel="noopener"
				class="border border-border px-4 py-2.5 text-sm text-text-secondary transition-colors hover:border-accent hover:text-accent"
			>
				GitHub &rarr;
			</a>
		</div>
	</section>
	</div>
</div>

<style>
	.docs-section {
		scroll-margin-top: 2rem;
	}
</style>
