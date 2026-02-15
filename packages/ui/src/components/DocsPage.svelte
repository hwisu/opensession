<script lang="ts">
const {
	onNavigate = (_path: string) => {},
}: {
	onNavigate?: (path: string) => void;
} = $props();

type TocItem = {
	id: string;
	flag: string;
	title: string;
	children?: { id: string; title: string }[];
};

const sections: TocItem[] = [
	{ id: 'getting-started', flag: '--init', title: 'Getting Started' },
	{ id: 'sessions', flag: '--upload', title: 'Sessions' },
	{ id: 'teams', flag: '--team', title: 'Teams' },
	{ id: 'invitations', flag: '--invite', title: 'Invitations' },
	{ id: 'team-stats', flag: '--stats', title: 'Team Stats' },
	{
		id: 'cli',
		flag: '--cli',
		title: 'CLI Reference',
		children: [
			{ id: 'cli-config', title: 'config' },
			{ id: 'cli-upload', title: 'upload' },
			{ id: 'cli-handoff', title: 'handoff' },
			{ id: 'cli-daemon', title: 'daemon' },
			{ id: 'cli-stream', title: 'stream' },
			{ id: 'cli-completion', title: 'completion' },
			{ id: 'cli-refs', title: 'Session References' },
			{ id: 'cli-formats', title: 'Output Formats' },
		],
	},
	{ id: 'self-hosting', flag: '--host', title: 'Self-Hosting' },
	{ id: 'hail-format', flag: '--spec', title: 'HAIL Format' },
];

let activeId = $state('getting-started');
let docQuery = $state('');

function normalize(value: string): string {
	return value.trim().toLowerCase();
}

const filteredSections = $derived.by(() => {
	const query = normalize(docQuery);
	if (!query) return sections;

	const matches: TocItem[] = [];
	for (const section of sections) {
		const sectionMatch = [section.title, section.flag, section.id].some((value) =>
			value.toLowerCase().includes(query),
		);
		const childMatches = (section.children ?? []).filter((child) =>
			[child.title, child.id].some((value) => value.toLowerCase().includes(query)),
		);
		if (sectionMatch || childMatches.length > 0) {
			matches.push({
				...section,
				children: sectionMatch ? section.children : childMatches,
			});
		}
	}
	return matches;
});

const docMatchCount = $derived.by(() => {
	return filteredSections.reduce(
		(count, section) => count + 1 + (section.children?.length ?? 0),
		0,
	);
});

const firstDocMatchId = $derived.by(() => filteredSections[0]?.id ?? null);

function scrollTo(id: string) {
	document.getElementById(id)?.scrollIntoView({ behavior: 'smooth' });
}

function handleDocsSearchKeydown(e: KeyboardEvent) {
	if (e.key === 'Enter' && firstDocMatchId) {
		e.preventDefault();
		scrollTo(firstDocMatchId);
	}
}

$effect(() => {
	const ids = sections.flatMap((s) => [s.id, ...(s.children?.map((c) => c.id) ?? [])]);
	const observer = new IntersectionObserver(
		(entries) => {
			for (const entry of entries) {
				if (entry.isIntersecting) {
					activeId = entry.target.id;
				}
			}
		},
		{ rootMargin: '-10% 0px -80% 0px' },
	);
	for (const id of ids) {
		const el = document.getElementById(id);
		if (el) observer.observe(el);
	}
	return () => observer.disconnect();
});
</script>

<div class="docs-root xl:flex xl:justify-center">
	<!-- Floating TOC (xl+ only) -->
	<aside class="hidden xl:block xl:w-52 xl:shrink-0">
		<nav class="sticky top-4 max-h-[calc(100vh-8rem)] overflow-y-auto py-10 pr-4">
			<div class="mb-3 text-[10px] font-bold uppercase tracking-widest text-text-muted">
				Contents
			</div>
			{#if docQuery.trim() && docMatchCount === 0}
				<p class="py-2 text-[11px] text-text-muted">No matches</p>
			{/if}
			{#each filteredSections as sec}
				<button
					onclick={() => scrollTo(sec.id)}
					class="group flex w-full items-center gap-1.5 py-1 text-left text-xs transition-colors"
					class:text-accent={activeId === sec.id}
					class:text-text-secondary={activeId !== sec.id}
				>
					<span class="w-1 h-1 shrink-0 bg-current opacity-40 group-hover:opacity-100" class:opacity-100={activeId === sec.id}></span>
					<span class="truncate group-hover:text-accent">{sec.title}</span>
				</button>
				{#if sec.children}
					{#each sec.children as child}
						<button
							onclick={() => scrollTo(child.id)}
							class="group flex w-full items-center gap-1.5 py-0.5 pl-3 text-left text-[11px] transition-colors"
							class:text-accent={activeId === child.id}
							class:text-text-muted={activeId !== child.id}
						>
							<span class="truncate group-hover:text-accent">{child.title}</span>
						</button>
					{/each}
				{/if}
			{/each}
		</nav>
	</aside>

	<!-- Main content -->
	<div class="min-w-0 max-w-4xl flex-1 px-6 py-10">

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
		<div class="mt-4 max-w-xl border border-border bg-bg-secondary px-3 py-2">
			<label for="docs-search" class="mb-1 block text-[11px] uppercase tracking-wider text-text-muted">
				Quick Search
			</label>
			<input
				id="docs-search"
				type="text"
				bind:value={docQuery}
				onkeydown={handleDocsSearchKeydown}
				placeholder="Search by section/command (e.g. upload, oauth, daemon)"
				class="w-full bg-transparent text-sm text-text-primary placeholder-text-muted outline-none"
			/>
			<p class="mt-1 text-[11px] text-text-muted">
				{#if docQuery.trim()}
					{docMatchCount} match{docMatchCount === 1 ? '' : 'es'} · press Enter to jump
				{:else}
					Search filters the table of contents
				{/if}
			</p>
		</div>
	</div>

	<!-- Inline Table of Contents (below xl) -->
	<nav class="mb-12 border border-border p-5 xl:hidden">
		<div class="mb-3 text-xs font-bold uppercase tracking-wider text-text-muted">Contents</div>
		<div class="grid gap-1 sm:grid-cols-2">
			{#each filteredSections as sec}
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
			<h3 class="text-sm font-bold text-text-primary">Landing Page (Signed Out)</h3>
			<p>
				At <button onclick={() => onNavigate('/')} class="text-accent hover:underline">/</button>,
				signed-out users see the landing page. After sign-in, the same route shows your session list.
			</p>

			<h3 class="text-sm font-bold text-text-primary">Create an Account</h3>
			<p>
				Sign up at <button onclick={() => onNavigate('/register')} class="text-accent hover:underline">/register</button> with
				a username and password, or use OAuth (GitHub, Google) if available on your instance.
			</p>

			<h3 class="text-sm font-bold text-text-primary">Sign In</h3>
			<p>
				Log in at <button onclick={() => onNavigate('/login')} class="text-accent hover:underline">/login</button>.
				Password and OAuth methods are both supported. Sign-in is required for uploads and account settings.
			</p>

			<h3 class="text-sm font-bold text-text-primary">Deployment Profiles</h3>
			<div class="grid gap-px border border-border bg-border sm:grid-cols-2">
				<div class="bg-bg-primary p-3">
					<p class="mb-1 text-xs font-bold text-accent">Docker profile</p>
					<p class="text-xs">Team-focused. Teams/invitations UI and team APIs are enabled.</p>
				</div>
				<div class="bg-bg-primary p-3">
					<p class="mb-1 text-xs font-bold text-accent">Worker profile</p>
					<p class="text-xs">Personal-sharing focused. Team UI/API is disabled.</p>
				</div>
			</div>

			<h3 class="text-sm font-bold text-text-primary">Get Your API Key</h3>
			<p>
				Navigate to <button onclick={() => onNavigate('/settings')} class="text-accent hover:underline">/settings</button> to
				find your API key (starts with <code class="text-accent">osk_</code>). You'll need this for uploading sessions via the CLI.
			</p>

			<div class="grid gap-px border border-border bg-border sm:grid-cols-2">
				<div class="bg-bg-primary p-3">
					<p class="mb-1 text-xs font-bold text-accent">Guest (not signed in)</p>
					<p class="text-xs">See landing page and docs. Sign in to access account and upload actions.</p>
				</div>
				<div class="bg-bg-primary p-3">
					<p class="mb-1 text-xs font-bold text-accent">Signed in</p>
					<p class="text-xs">Session list, uploads, settings, and profile-specific collaboration features.</p>
				</div>
			</div>
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
				<code class="block text-xs text-accent">$ opensession publish upload ./session.jsonl</code>
				<code class="mt-1 block text-xs text-accent">$ opensession publish upload-all</code>
			</div>

			<div class="border border-border bg-bg-secondary p-4">
				<div class="mb-2 text-xs uppercase tracking-wider text-text-muted">Web Upload</div>
				<p class="text-xs">
					Drag and drop session <code class="text-accent">.jsonl</code> files onto the
					<button onclick={() => onNavigate('/upload')} class="text-accent hover:underline">/upload</button> page,
					or click to select files. Docker profile supports team-target upload, while Worker profile uses personal mode.
				</p>
			</div>

			<h3 class="text-sm font-bold text-text-primary">Viewing Sessions</h3>
			<p>
				Your session list at <button onclick={() => onNavigate('/')} class="text-accent hover:underline">/</button> shows
				your sessions after sign-in. Each card displays the session tool, model, timestamp, token count, and a preview of the conversation.
			</p>

			<h3 class="text-sm font-bold text-text-primary">Timeline View</h3>
			<p>
				Click into any session to see the full timeline — every message, tool call, file edit,
				and error in chronological order. Use the sidebar to jump between events or filter by type.
			</p>

			<h3 class="text-sm font-bold text-text-primary">Search &amp; Filter</h3>
			<p>
				Use the search bar (<kbd class="border border-border bg-bg-secondary px-1.5 py-0.5 text-xs">/</kbd> to focus)
				to search across all sessions by content. Filter by tool/model, and by team when the Docker profile is enabled.
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
			<p class="text-xs text-text-muted">Team pages require sign-in.</p>

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
			<p class="text-xs text-text-muted">Inbox is account-scoped, so sign-in is required.</p>

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
			<p>See how many sessions, tokens, and tool calls each team member has contributed.</p>

			<h3 class="text-sm font-bold text-text-primary">By Tool</h3>
			<p>Break down usage by AI tool — see which tools your team uses most (Claude Code, Cursor, Codex, etc.).</p>

			<h3 class="text-sm font-bold text-text-primary">Time Ranges</h3>
			<p>Filter stats by time range to see usage trends over the past week, month, or custom date ranges.</p>
		</div>
	</section>

	<!-- ═══ CLI Reference ═══ -->
	<section id="cli" class="docs-section mb-12">
		<div class="mb-1 text-xs text-accent">--cli</div>
		<h2 class="mb-4 text-lg font-bold text-text-primary">CLI Reference</h2>

		<div class="space-y-6 text-sm leading-relaxed text-text-secondary">

			<!-- Installation -->
			<div>
				<h3 class="text-sm font-bold text-text-primary">Installation</h3>
				<div class="mt-2 border border-border bg-bg-secondary p-4">
					<code class="block text-xs text-accent">$ cargo install opensession</code>
				</div>
				<p class="mt-2">
					Running <code class="text-accent">opensession</code> without arguments launches the TUI.
					Subcommands run CLI operations.
				</p>
			</div>

			<!-- config -->
			<div id="cli-config" class="docs-section border-t border-border pt-5">
				<h3 class="mb-1 text-sm font-bold text-text-primary">
					<code class="text-accent">opensession account connect / team / status</code>
				</h3>
				<p>Connect server/API key/team quickly and verify connectivity.</p>

				<div class="mt-3 space-y-2">
					<div class="cli-flags">
						<code>account connect --server &lt;URL&gt;</code> <span>Server URL</span>
					</div>
					<div class="cli-flags">
						<code>account connect --api-key &lt;KEY&gt;</code> <span>API key (<code class="text-accent">osk_</code> prefix)</span>
					</div>
					<div class="cli-flags">
						<code>account team --id &lt;ID&gt;</code> <span>Default team ID</span>
					</div>
				</div>

				<div class="mt-3 border border-border bg-bg-secondary p-4">
					<div class="mb-2 text-[10px] uppercase tracking-wider text-text-muted">Examples</div>
					<pre class="text-xs text-accent leading-relaxed"><span class="text-text-muted"># Show current configuration</span>
$ opensession account show

<span class="text-text-muted"># Set server URL and API key</span>
$ opensession account connect --server https://opensession.io --api-key osk_abc123

<span class="text-text-muted"># Set default team for uploads</span>
$ opensession account team --id my-team

<span class="text-text-muted"># Check server/auth</span>
$ opensession account status
$ opensession account verify</pre>
				</div>
				<p class="mt-2 text-xs text-text-muted">Config file: <code class="text-accent">~/.config/opensession/opensession.toml</code></p>
			</div>

			<!-- upload / upload-all -->
			<div id="cli-upload" class="docs-section border-t border-border pt-5">
				<h3 class="mb-1 text-sm font-bold text-text-primary">
					<code class="text-accent">opensession publish upload</code> /
					<code class="text-accent">opensession publish upload-all</code>
				</h3>
				<p>Upload session files to the server.</p>

				<div class="mt-3 space-y-2">
					<div class="cli-flags">
						<code>&lt;file&gt;</code> <span>Path to session file (required for <code class="text-accent">upload</code>)</span>
					</div>
					<div class="cli-flags">
						<code>--parent &lt;ID&gt;</code> <span>Link to parent session(s), repeatable</span>
					</div>
					<div class="cli-flags">
						<code>--git</code> <span>Store on git branch instead of server</span>
					</div>
				</div>

				<div class="mt-3 border border-border bg-bg-secondary p-4">
					<div class="mb-2 text-[10px] uppercase tracking-wider text-text-muted">Examples</div>
					<pre class="text-xs text-accent leading-relaxed"><span class="text-text-muted"># Upload a single session</span>
$ opensession publish upload ./session.jsonl

<span class="text-text-muted"># Upload with parent session linkage</span>
$ opensession publish upload ./followup.jsonl --parent abc123

<span class="text-text-muted"># Discover and upload all sessions at once</span>
$ opensession publish upload-all

<span class="text-text-muted"># Store session in git branch instead of server</span>
$ opensession publish upload ./session.jsonl --git</pre>
				</div>
				<p class="mt-2 text-xs text-text-muted">
					<code class="text-accent">upload-all</code> skips subagent files and already-uploaded sessions automatically.
				</p>
			</div>

			<!-- handoff -->
			<div id="cli-handoff" class="docs-section border-t border-border pt-5">
				<h3 class="mb-1 text-sm font-bold text-text-primary">
					<code class="text-accent">opensession session handoff</code>
				</h3>
				<p>Generate a session summary for handing off context to the next AI agent.</p>

				<div class="mt-3 space-y-2">
					<div class="cli-flags">
						<code>[files...]</code> <span>Session file path(s). Multiple files merge into one handoff</span>
					</div>
					<div class="cli-flags">
						<code>-l, --last</code> <span>Use the most recent session</span>
					</div>
					<div class="cli-flags">
						<code>--claude &lt;REF&gt;</code> <span>Claude Code session reference (HEAD, HEAD~2)</span>
					</div>
					<div class="cli-flags">
						<code>--gemini &lt;REF&gt;</code> <span>Gemini session reference</span>
					</div>
					<div class="cli-flags">
						<code>--tool &lt;TOOL_REF&gt;</code> <span>Generic tool reference (e.g. "amp HEAD~2"), repeatable</span>
					</div>
					<div class="cli-flags">
						<code>-o, --output &lt;PATH&gt;</code> <span>Write to file instead of stdout</span>
					</div>
					<div class="cli-flags">
						<code>--format &lt;FMT&gt;</code> <span>Output format (default: markdown)</span>
					</div>
				</div>

					<div class="mt-3 border border-border bg-bg-secondary p-4">
						<div class="mb-2 text-[10px] uppercase tracking-wider text-text-muted">Examples</div>
						<pre class="text-xs text-accent leading-relaxed"><span class="text-text-muted"># Handoff from the last Claude Code session</span>
$ opensession session handoff --claude HEAD

<span class="text-text-muted"># Handoff from most recent session</span>
$ opensession session handoff --last

<span class="text-text-muted"># Merge multiple sessions into one handoff</span>
$ opensession session handoff session1.jsonl session2.jsonl

<span class="text-text-muted"># Save handoff to a file</span>
$ opensession session handoff --claude HEAD -o handoff.md</pre>
					</div>
				</div>

			<!-- daemon -->
			<div id="cli-daemon" class="docs-section border-t border-border pt-5">
				<h3 class="mb-1 text-sm font-bold text-text-primary">
					<code class="text-accent">opensession daemon</code>
				</h3>
				<p>Manage the background daemon that watches for new sessions and syncs them.</p>

				<div class="mt-3 space-y-2">
					<div class="cli-flags">
						<code>start</code> <span>Start the background daemon</span>
					</div>
					<div class="cli-flags">
						<code>stop</code> <span>Stop the daemon</span>
					</div>
					<div class="cli-flags">
						<code>status</code> <span>Show daemon status</span>
					</div>
					<div class="cli-flags">
						<code>health</code> <span>Check daemon and server health</span>
					</div>
					<div class="cli-flags">
						<code>select --agent ... --repo ...</code> <span>Update watcher targets without starting daemon</span>
					</div>
					<div class="cli-flags">
						<code>show</code> <span>Show current watcher targets</span>
					</div>
				</div>

				<div class="mt-3 border border-border bg-bg-secondary p-4">
					<div class="mb-2 text-[10px] uppercase tracking-wider text-text-muted">Examples</div>
					<pre class="text-xs text-accent leading-relaxed"><span class="text-text-muted"># Start the daemon in the background</span>
$ opensession daemon start

<span class="text-text-muted"># Check if daemon is running</span>
$ opensession daemon status

<span class="text-text-muted"># Verify daemon + server connectivity</span>
$ opensession daemon health

<span class="text-text-muted"># Stop the daemon</span>
$ opensession daemon stop

<span class="text-text-muted"># Select agents/repos to watch</span>
$ opensession daemon select --agent claude-code --repo .</pre>
				</div>

				<p class="mt-3 text-xs text-text-muted">
					The daemon watches for new sessions from configured tools and syncs them to the server.
					Configure via <code class="text-accent">~/.config/opensession/opensession.toml</code> or the TUI settings.
				</p>

				<div class="mt-3 border border-border bg-bg-secondary p-4">
					<div class="mb-2 text-[10px] uppercase tracking-wider text-text-muted">opensession.toml</div>
					<pre class="text-xs text-accent leading-relaxed"><span class="text-text-muted">[daemon]</span>
auto_publish = false         <span class="text-text-muted"># managed by TUI "Daemon Capture" toggle</span>
publish_on = "manual"        <span class="text-text-muted"># ON =&gt; session_end, OFF =&gt; manual</span>
debounce_secs = 5

<span class="text-text-muted">[watchers]</span>
custom_paths = [
  "~/.claude/projects",
  "~/.codex/sessions",
  "~/.local/share/opencode/storage/session",
]

<span class="text-text-muted">[privacy]</span>
strip_paths = true
strip_env_vars = true</pre>
				</div>
				<p class="mt-2 text-xs text-text-muted">
					Legacy per-agent watcher toggles are parsed for backward compatibility, but new saves write
					<code class="text-accent">watchers.custom_paths</code> only.
				</p>
			</div>

			<!-- stream -->
			<div id="cli-stream" class="docs-section border-t border-border pt-5">
				<h3 class="mb-1 text-sm font-bold text-text-primary">
					<code class="text-accent">opensession daemon stream-push</code>
				</h3>
				<p>Internal hook target command for daemon streaming (normally invoked automatically).</p>
				<div class="mt-3 border border-border bg-bg-secondary p-4">
					<div class="mb-2 text-[10px] uppercase tracking-wider text-text-muted">Example</div>
					<pre class="text-xs text-accent leading-relaxed">$ opensession daemon stream-push --agent claude-code</pre>
				</div>
			</div>

			<!-- completion -->
			<div id="cli-completion" class="docs-section border-t border-border pt-5">
				<h3 class="mb-1 text-sm font-bold text-text-primary">
					<code class="text-accent">opensession docs completion</code>
				</h3>

				<div class="mt-3 border border-border bg-bg-secondary p-4">
					<div class="mb-2 text-[10px] uppercase tracking-wider text-text-muted">Examples</div>
					<pre class="text-xs text-accent leading-relaxed"><span class="text-text-muted"># Generate shell completions</span>
$ opensession docs completion bash >> ~/.bashrc
$ opensession docs completion zsh >> ~/.zshrc
$ opensession docs completion fish > ~/.config/fish/completions/opensession.fish</pre>
				</div>
			</div>

			<!-- Session References -->
			<div id="cli-refs" class="docs-section border-t border-border pt-5">
				<h3 class="mb-2 text-sm font-bold text-text-primary">Session References</h3>
				<p>
					The <code class="text-accent">handoff</code> command accepts flexible session references:
				</p>

				<div class="mt-3 grid gap-px border border-border bg-border">
					<div class="flex bg-bg-secondary px-4 py-2 text-xs">
						<code class="w-28 shrink-0 font-bold text-accent">HEAD</code>
						<span>Latest session</span>
					</div>
					<div class="flex bg-bg-primary px-4 py-2 text-xs">
						<code class="w-28 shrink-0 font-bold text-accent">HEAD~N</code>
						<span>Latest N sessions (merged)</span>
					</div>
					<div class="flex bg-bg-secondary px-4 py-2 text-xs">
						<code class="w-28 shrink-0 font-bold text-accent">HEAD^N</code>
						<span>Nth most recent session (0-indexed)</span>
					</div>
					<div class="flex bg-bg-primary px-4 py-2 text-xs">
						<code class="w-28 shrink-0 font-bold text-accent">&lt;id&gt;</code>
						<span>Session ID (prefix matching supported)</span>
					</div>
					<div class="flex bg-bg-secondary px-4 py-2 text-xs">
						<code class="w-28 shrink-0 font-bold text-accent">&lt;path&gt;</code>
						<span>Path to a session file</span>
					</div>
				</div>

				<div class="mt-3 border border-border bg-bg-secondary p-4">
					<div class="mb-2 text-[10px] uppercase tracking-wider text-text-muted">Examples</div>
					<pre class="text-xs text-accent leading-relaxed"><span class="text-text-muted"># Last Claude Code session</span>
$ opensession session handoff --claude HEAD

<span class="text-text-muted"># Last 3 Claude Code sessions merged</span>
$ opensession session handoff --claude HEAD~3

<span class="text-text-muted"># Reference by ID prefix</span>
$ opensession session handoff abc12</pre>
				</div>
			</div>

			<!-- Output Formats -->
			<div id="cli-formats" class="docs-section border-t border-border pt-5">
				<h3 class="mb-2 text-sm font-bold text-text-primary">Output Formats</h3>
				<p>
					Available via <code class="text-accent">opensession session handoff --format ...</code>:
				</p>

				<div class="mt-3 grid gap-px border border-border bg-border">
					<div class="flex bg-bg-secondary px-4 py-2 text-xs">
						<code class="w-24 shrink-0 font-bold text-accent">text</code>
						<span>Human-readable plain text</span>
					</div>
					<div class="flex bg-bg-primary px-4 py-2 text-xs">
						<code class="w-24 shrink-0 font-bold text-accent">markdown</code>
						<span>Markdown format (default for handoff)</span>
					</div>
					<div class="flex bg-bg-secondary px-4 py-2 text-xs">
						<code class="w-24 shrink-0 font-bold text-accent">json</code>
						<span>JSON format</span>
					</div>
					<div class="flex bg-bg-primary px-4 py-2 text-xs">
						<code class="w-24 shrink-0 font-bold text-accent">jsonl</code>
						<span>JSONL (one JSON object per line)</span>
					</div>
					<div class="flex bg-bg-secondary px-4 py-2 text-xs">
						<code class="w-24 shrink-0 font-bold text-accent">hail</code>
						<span>HAIL session format</span>
					</div>
					<div class="flex bg-bg-primary px-4 py-2 text-xs">
						<code class="w-24 shrink-0 font-bold text-accent">stream</code>
						<span>NDJSON stream</span>
					</div>
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
				<code class="block text-xs text-accent">$ docker run -p 3000:3000 -v opensession-data:/data -e JWT_SECRET=your-secret ghcr.io/hwisu/opensession</code>
			</div>
			<p>
				This starts the server on port 3000 with an embedded SQLite database and persistent storage.
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
      - JWT_SECRET=your-secret-here
      - BASE_URL=https://your-domain.com
      - OPENSESSION_PUBLIC_FEED_ENABLED=false
    restart: unless-stopped

volumes:
  opensession-data:</pre>
			</div>

			<h3 class="text-sm font-bold text-text-primary">Environment Variables</h3>
				<div class="grid gap-px border border-border bg-border">
					<div class="flex bg-bg-secondary px-4 py-2 text-xs">
						<code class="w-48 shrink-0 font-bold text-accent">JWT_SECRET</code>
						<span>Secret for JWT token signing (required)</span>
					</div>
					<div class="flex bg-bg-primary px-4 py-2 text-xs">
						<code class="w-48 shrink-0 font-bold text-accent">OPENSESSION_DATA_DIR</code>
						<span>SQLite DB and session storage (default: data/)</span>
					</div>
					<div class="flex bg-bg-secondary px-4 py-2 text-xs">
						<code class="w-48 shrink-0 font-bold text-accent">BASE_URL</code>
						<span>Public-facing URL (default: http://localhost:3000)</span>
					</div>
					<div class="flex bg-bg-primary px-4 py-2 text-xs">
						<code class="w-48 shrink-0 font-bold text-accent">OPENSESSION_PUBLIC_FEED_ENABLED</code>
						<span>Set false to block anonymous <code class="text-accent">GET /api/sessions</code></span>
					</div>
					<div class="flex bg-bg-secondary px-4 py-2 text-xs">
						<code class="w-48 shrink-0 font-bold text-accent">PORT</code>
						<span>HTTP listen port (default: 3000)</span>
					</div>
				</div>

			<h3 class="text-sm font-bold text-text-primary">Point the CLI to Your Instance</h3>
			<div class="border border-border bg-bg-secondary p-4">
				<code class="block text-xs text-accent">$ opensession account connect --server http://localhost:3000</code>
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
			<p>Every HAIL file is JSONL: header line, event lines, then one stats line.</p>
			<div class="border border-border bg-bg-secondary p-4">
				<pre class="text-xs leading-relaxed"><span class="text-text-muted">// Line 1: Session metadata</span>
<span class="text-accent">{`{"type":"header","version":"hail-1.0.0","session_id":"...","agent":{"tool":"codex","provider":"openai","model":"gpt-5"},"context":{"created_at":"...","updated_at":"..."}}`}</span>

<span class="text-text-muted">// Line 2..N: Events</span>
<span class="text-role-human">{`{"type":"event","event_id":"e1","timestamp":"...","event_type":{"type":"UserMessage"},"content":{"blocks":[{"type":"Text","text":"Fix auth"}]}}`}</span>
<span class="text-role-tool">{`{"type":"event","event_id":"e2","timestamp":"...","event_type":{"type":"ToolCall","data":{"name":"edit_file"}},"content":{"blocks":[{"type":"Text","text":"Editing src/auth.rs"}]}}`}</span>

<span class="text-text-muted">// Last line: Aggregate stats</span>
<span class="text-role-agent">{`{"type":"stats","event_count":2,"message_count":1,"tool_call_count":1}`}</span></pre>
			</div>

			<h3 class="text-sm font-bold text-text-primary">Event Types</h3>
			<div class="grid gap-px border border-border bg-border sm:grid-cols-2">
				<div class="bg-bg-primary p-3">
					<span class="text-xs font-bold text-role-human">UserMessage</span>
					<p class="mt-0.5 text-xs">Human prompts/messages</p>
				</div>
				<div class="bg-bg-primary p-3">
					<span class="text-xs font-bold text-role-agent">AgentMessage</span>
					<p class="mt-0.5 text-xs">Assistant output/reasoning</p>
				</div>
				<div class="bg-bg-primary p-3">
					<span class="text-xs font-bold text-role-tool">ToolCall / FileEdit</span>
					<p class="mt-0.5 text-xs">Tool activity and file changes</p>
				</div>
				<div class="bg-bg-primary p-3">
					<span class="text-xs font-bold text-error">TaskStart / TaskEnd / Error-like custom</span>
					<p class="mt-0.5 text-xs">Task boundaries and custom events</p>
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
				href="https://github.com/hwisu/opensession"
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
	.cli-flags {
		display: flex;
		gap: 0.75rem;
		align-items: baseline;
		font-size: 12px;
		padding: 3px 0;
	}
	.cli-flags code {
		flex-shrink: 0;
		color: var(--color-accent);
		font-weight: 600;
		font-size: 11px;
	}
	.cli-flags span {
		color: var(--color-text-muted);
	}
</style>
