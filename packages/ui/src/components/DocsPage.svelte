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
			{ id: 'cli-discover', title: 'discover' },
			{ id: 'cli-view', title: 'view' },
			{ id: 'cli-upload', title: 'upload' },
			{ id: 'cli-log', title: 'log' },
			{ id: 'cli-stats', title: 'stats' },
			{ id: 'cli-handoff', title: 'handoff' },
			{ id: 'cli-diff', title: 'diff' },
			{ id: 'cli-daemon', title: 'daemon' },
			{ id: 'cli-server', title: 'server' },
			{ id: 'cli-hooks', title: 'hooks' },
			{ id: 'cli-stream', title: 'stream' },
			{ id: 'cli-misc', title: 'index & completion' },
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
				Password and OAuth methods are both supported. Sign-in is required for teams, inbox, uploads, and account settings.
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
					Drag and drop parsed session <code class="text-accent">.json</code> files onto the
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
					<code class="text-accent">opensession account config</code>
				</h3>
				<p>Show or set CLI configuration.</p>

				<div class="mt-3 space-y-2">
					<div class="cli-flags">
						<code>--server &lt;URL&gt;</code> <span>Server URL</span>
					</div>
					<div class="cli-flags">
						<code>--api-key &lt;KEY&gt;</code> <span>API key (<code class="text-accent">osk_</code> prefix)</span>
					</div>
					<div class="cli-flags">
						<code>--team-id &lt;ID&gt;</code> <span>Default team ID</span>
					</div>
				</div>

				<div class="mt-3 border border-border bg-bg-secondary p-4">
					<div class="mb-2 text-[10px] uppercase tracking-wider text-text-muted">Examples</div>
					<pre class="text-xs text-accent leading-relaxed"><span class="text-text-muted"># Show current configuration</span>
$ opensession account config

<span class="text-text-muted"># Set server URL and API key</span>
$ opensession account config --server https://opensession.io --api-key osk_abc123

<span class="text-text-muted"># Set default team for uploads</span>
$ opensession account config --team-id my-team</pre>
				</div>
				<p class="mt-2 text-xs text-text-muted">Config file: <code class="text-accent">~/.config/opensession/config.toml</code></p>
			</div>

			<!-- discover -->
			<div id="cli-discover" class="docs-section border-t border-border pt-5">
				<h3 class="mb-1 text-sm font-bold text-text-primary">
					<code class="text-accent">opensession session discover</code>
				</h3>
				<p>Scan this machine for AI sessions from all supported tools.</p>

				<div class="mt-3 border border-border bg-bg-secondary p-4">
					<div class="mb-2 text-[10px] uppercase tracking-wider text-text-muted">Example</div>
					<pre class="text-xs text-accent leading-relaxed">$ opensession session discover

<span class="text-text-muted">Found 47 sessions:</span>
<span class="text-tool-claude">  claude-code</span>  <span class="text-text-muted">32 sessions  ~/.claude/projects/</span>
<span class="text-tool-cursor">  cursor</span>       <span class="text-text-muted"> 8 sessions  ~/.cursor/</span>
<span class="text-tool-codex">  codex</span>        <span class="text-text-muted"> 4 sessions  ~/.codex/sessions/</span>
<span class="text-tool-opencode">  opencode</span>     <span class="text-text-muted"> 3 sessions  ~/.local/share/opencode/</span></pre>
				</div>
				<p class="mt-2 text-xs text-text-muted">Supported: Claude Code, Cursor, Codex, OpenCode, Cline, Amp, Gemini</p>
			</div>

			<!-- view -->
			<div id="cli-view" class="docs-section border-t border-border pt-5">
				<h3 class="mb-1 text-sm font-bold text-text-primary">
					<code class="text-accent">opensession view &lt;agent&gt;</code>
				</h3>
				<p>Open a real-time Session Detail view focused on one active agent session.</p>

				<div class="mt-3 space-y-2">
					<div class="cli-flags">
						<code>&lt;agent&gt;</code> <span>claude | codex | cursor | gemini | opencode | cline | amp</span>
					</div>
					<div class="cli-flags">
						<code>--active-within-minutes &lt;N&gt;</code> <span>Active window by file mtime (default: 20)</span>
					</div>
					<div class="cli-flags">
						<code>--latest</code> <span>Ignore active window and always choose latest</span>
					</div>
					<div class="cli-flags">
						<code>--non-interactive</code> <span>Skip picker and auto-select latest candidate</span>
					</div>
					<div class="cli-flags">
						<code>--dry-run</code> <span>Print selected session/runtime overrides and exit</span>
					</div>
					<div class="cli-flags">
						<code>--summary-provider &lt;PROVIDER&gt;</code> <span>Runtime-only summary provider override</span>
					</div>
					<div class="cli-flags">
						<code>--summary-model &lt;MODEL&gt;</code> <span>Runtime-only model override</span>
					</div>
					<div class="cli-flags">
						<code>--sum-endpoint / --sum-base / --sum-path</code> <span>OpenAI-compatible endpoint overrides</span>
					</div>
					<div class="cli-flags">
						<code>--sum-style &lt;chat|responses&gt;</code> <span>OpenAI-compatible payload style</span>
					</div>
					<div class="cli-flags">
						<code>--sum-key / --sum-key-header</code> <span>Runtime-only API key/header override</span>
					</div>
				</div>

				<div class="mt-3 border border-border bg-bg-secondary p-4">
					<div class="mb-2 text-[10px] uppercase tracking-wider text-text-muted">Examples</div>
					<pre class="text-xs text-accent leading-relaxed"><span class="text-text-muted"># Open active Claude session in live detail view</span>
$ opensession view claude

<span class="text-text-muted"># Prefer latest only, no picker</span>
$ opensession view codex --latest --non-interactive

<span class="text-text-muted"># Dry-run selection and runtime summary settings</span>
$ opensession view cursor --dry-run --non-interactive

<span class="text-text-muted"># Runtime summary API override (not persisted)</span>
$ opensession view gemini --summary-provider openai-compatible --sum-endpoint https://example.com/v1/responses --sum-style responses</pre>
				</div>
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

			<!-- log -->
			<div id="cli-log" class="docs-section border-t border-border pt-5">
				<h3 class="mb-1 text-sm font-bold text-text-primary">
					<code class="text-accent">opensession session log</code>
				</h3>
				<p>Show session history in a git-log style format.</p>

				<div class="mt-3 space-y-2">
					<div class="cli-flags">
						<code>--since &lt;TIME&gt;</code> <span>Filter by time (e.g. "3 hours", "2 days", "1 week")</span>
					</div>
					<div class="cli-flags">
						<code>--before &lt;TIME&gt;</code> <span>Show sessions before this time</span>
					</div>
					<div class="cli-flags">
						<code>--tool &lt;TOOL&gt;</code> <span>Filter by tool (e.g. "claude-code", "cursor")</span>
					</div>
					<div class="cli-flags">
						<code>--model &lt;MODEL&gt;</code> <span>Filter by model (supports wildcards: "opus*")</span>
					</div>
					<div class="cli-flags">
						<code>--grep &lt;QUERY&gt;</code> <span>Search in titles and descriptions</span>
					</div>
					<div class="cli-flags">
						<code>--touches &lt;FILE&gt;</code> <span>Show sessions that touched a specific file</span>
					</div>
					<div class="cli-flags">
						<code>--has-errors</code> <span>Show only sessions with errors</span>
					</div>
					<div class="cli-flags">
						<code>--project &lt;PATH&gt;</code> <span>Filter by working directory</span>
					</div>
					<div class="cli-flags">
						<code>-n, --limit &lt;N&gt;</code> <span>Max results (default: 20)</span>
					</div>
					<div class="cli-flags">
						<code>--format &lt;FMT&gt;</code> <span>Output format (text, json, jsonl, markdown)</span>
					</div>
					<div class="cli-flags">
						<code>--json [FIELDS]</code> <span>Select JSON fields (e.g. "id,tool,title")</span>
					</div>
					<div class="cli-flags">
						<code>--jq &lt;FILTER&gt;</code> <span>Apply jq filter to JSON output</span>
					</div>
				</div>

				<div class="mt-3 border border-border bg-bg-secondary p-4">
					<div class="mb-2 text-[10px] uppercase tracking-wider text-text-muted">Examples</div>
					<pre class="text-xs text-accent leading-relaxed"><span class="text-text-muted"># Show recent sessions</span>
$ opensession session log

<span class="text-text-muted"># Sessions from the last 3 hours</span>
$ opensession session log --since "3 hours"

<span class="text-text-muted"># Only Claude Code sessions with errors</span>
$ opensession session log --tool claude-code --has-errors

<span class="text-text-muted"># Search for sessions about authentication</span>
$ opensession session log --grep "auth" --limit 5

<span class="text-text-muted"># Sessions that touched a specific file</span>
$ opensession session log --touches src/auth.rs

<span class="text-text-muted"># Filter by model using wildcards</span>
$ opensession session log --model "opus*"

<span class="text-text-muted"># Export as JSON with specific fields</span>
$ opensession session log --json "id,tool,title,created_at"

<span class="text-text-muted"># Pipe through jq for custom queries</span>
$ opensession session log --format json --jq '.[] | select(.has_errors)'</pre>
				</div>
			</div>

			<!-- stats -->
			<div id="cli-stats" class="docs-section border-t border-border pt-5">
				<h3 class="mb-1 text-sm font-bold text-text-primary">
					<code class="text-accent">opensession session stats</code>
				</h3>
				<p>Show AI usage statistics — sessions, tokens, costs, and breakdowns by tool.</p>

				<div class="mt-3 space-y-2">
					<div class="cli-flags">
						<code>--period &lt;PERIOD&gt;</code> <span>Time period: day, week (default), month, all</span>
					</div>
					<div class="cli-flags">
						<code>--format &lt;FMT&gt;</code> <span>Output format: text (default), json</span>
					</div>
				</div>

				<div class="mt-3 border border-border bg-bg-secondary p-4">
					<div class="mb-2 text-[10px] uppercase tracking-wider text-text-muted">Examples</div>
					<pre class="text-xs text-accent leading-relaxed"><span class="text-text-muted"># This week's stats</span>
$ opensession session stats

<span class="text-text-muted"># All-time usage</span>
$ opensession session stats --period all

<span class="text-text-muted"># Today's stats in JSON</span>
$ opensession session stats --period day --format json</pre>
				</div>
				<p class="mt-2 text-xs text-text-muted">
					Shows: total sessions, duration, token counts (input/output),
					breakdown by tool, top edited files, error rate, and estimated cost.
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
						<code>--summarize</code> <span>Generate LLM-powered summary</span>
					</div>
					<div class="cli-flags">
						<code>--ai &lt;PROVIDER&gt;</code> <span>AI provider for summarization: claude, openai, gemini</span>
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

<span class="text-text-muted"># Handoff with AI-powered summary</span>
$ opensession session handoff --last --summarize

<span class="text-text-muted"># Merge multiple sessions into one handoff</span>
$ opensession session handoff session1.jsonl session2.jsonl

<span class="text-text-muted"># Save handoff to a file</span>
$ opensession session handoff --claude HEAD -o handoff.md

<span class="text-text-muted"># Cross-tool handoff: Claude to Gemini</span>
$ opensession session handoff --claude HEAD~3 --summarize --ai gemini</pre>
				</div>
			</div>

			<!-- diff -->
			<div id="cli-diff" class="docs-section border-t border-border pt-5">
				<h3 class="mb-1 text-sm font-bold text-text-primary">
					<code class="text-accent">opensession session diff</code>
				</h3>
				<p>Compare two sessions side-by-side.</p>

				<div class="mt-3 space-y-2">
					<div class="cli-flags">
						<code>&lt;session_a&gt;</code> <span>First session (ID, file path, or reference)</span>
					</div>
					<div class="cli-flags">
						<code>&lt;session_b&gt;</code> <span>Second session</span>
					</div>
					<div class="cli-flags">
						<code>--ai</code> <span>Use AI to analyze differences</span>
					</div>
				</div>

				<div class="mt-3 border border-border bg-bg-secondary p-4">
					<div class="mb-2 text-[10px] uppercase tracking-wider text-text-muted">Examples</div>
					<pre class="text-xs text-accent leading-relaxed"><span class="text-text-muted"># Compare two sessions by file path</span>
$ opensession session diff ./before.jsonl ./after.jsonl

<span class="text-text-muted"># Compare using session references</span>
$ opensession session diff HEAD^2 HEAD^1

<span class="text-text-muted"># AI-powered diff analysis</span>
$ opensession session diff HEAD^2 HEAD^1 --ai</pre>
				</div>
			</div>

			<!-- daemon -->
			<div id="cli-daemon" class="docs-section border-t border-border pt-5">
				<h3 class="mb-1 text-sm font-bold text-text-primary">
					<code class="text-accent">opensession ops daemon</code>
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
				</div>

				<div class="mt-3 border border-border bg-bg-secondary p-4">
					<div class="mb-2 text-[10px] uppercase tracking-wider text-text-muted">Examples</div>
					<pre class="text-xs text-accent leading-relaxed"><span class="text-text-muted"># Start the daemon in the background</span>
$ opensession ops daemon start

<span class="text-text-muted"># Check if daemon is running</span>
$ opensession ops daemon status

<span class="text-text-muted"># Verify daemon + server connectivity</span>
$ opensession ops daemon health

<span class="text-text-muted"># Stop the daemon</span>
$ opensession ops daemon stop</pre>
				</div>

				<p class="mt-3 text-xs text-text-muted">
					The daemon watches for new sessions from configured tools and syncs them to the server.
					Configure via <code class="text-accent">~/.config/opensession/daemon.toml</code> or the TUI settings.
				</p>

				<div class="mt-3 border border-border bg-bg-secondary p-4">
					<div class="mb-2 text-[10px] uppercase tracking-wider text-text-muted">daemon.toml</div>
					<pre class="text-xs text-accent leading-relaxed"><span class="text-text-muted">[daemon]</span>
publish_on = "manual"        <span class="text-text-muted"># session_end | realtime | manual</span>
debounce_secs = 5

<span class="text-text-muted">[watchers]</span>
claude_code = true
opencode = true
cursor = false

<span class="text-text-muted">[privacy]</span>
strip_paths = true
strip_env_vars = true</pre>
				</div>
			</div>

			<!-- server -->
			<div id="cli-server" class="docs-section border-t border-border pt-5">
				<h3 class="mb-1 text-sm font-bold text-text-primary">
					<code class="text-accent">opensession account server</code>
				</h3>
				<p>Check server connection and authentication.</p>

				<div class="mt-3 space-y-2">
					<div class="cli-flags">
						<code>status</code> <span>Check server health and version</span>
					</div>
					<div class="cli-flags">
						<code>verify</code> <span>Verify API key authentication</span>
					</div>
				</div>

				<div class="mt-3 border border-border bg-bg-secondary p-4">
					<div class="mb-2 text-[10px] uppercase tracking-wider text-text-muted">Examples</div>
					<pre class="text-xs text-accent leading-relaxed"><span class="text-text-muted"># Check if server is reachable</span>
$ opensession account server status

<span class="text-text-muted"># Verify your API key works</span>
$ opensession account server verify</pre>
				</div>
			</div>

			<!-- hooks -->
			<div id="cli-hooks" class="docs-section border-t border-border pt-5">
				<h3 class="mb-1 text-sm font-bold text-text-primary">
					<code class="text-accent">opensession ops hooks</code>
				</h3>
				<p>Manage git hooks that link AI sessions to git commits.</p>

				<div class="mt-3 space-y-2">
					<div class="cli-flags">
						<code>install</code> <span>Install the prepare-commit-msg hook</span>
					</div>
					<div class="cli-flags">
						<code>uninstall</code> <span>Remove the hook</span>
					</div>
				</div>

				<div class="mt-3 border border-border bg-bg-secondary p-4">
					<div class="mb-2 text-[10px] uppercase tracking-wider text-text-muted">Examples</div>
					<pre class="text-xs text-accent leading-relaxed"><span class="text-text-muted"># Install in current repo</span>
$ opensession ops hooks install

<span class="text-text-muted"># Remove from current repo</span>
$ opensession ops hooks uninstall</pre>
				</div>
				<p class="mt-2 text-xs text-text-muted">
					When installed, the hook appends AI session metadata (tool, model, prompt) to your commit messages automatically.
				</p>
			</div>

			<!-- stream -->
			<div id="cli-stream" class="docs-section border-t border-border pt-5">
				<h3 class="mb-1 text-sm font-bold text-text-primary">
					<code class="text-accent">opensession ops stream</code>
				</h3>
				<p>Enable or disable real-time session streaming to the server.</p>

				<div class="mt-3 space-y-2">
					<div class="cli-flags">
						<code>enable [--agent &lt;AGENT&gt;]</code> <span>Enable streaming (auto-detects agent if omitted)</span>
					</div>
					<div class="cli-flags">
						<code>disable [--agent &lt;AGENT&gt;]</code> <span>Disable streaming</span>
					</div>
				</div>

				<div class="mt-3 border border-border bg-bg-secondary p-4">
					<div class="mb-2 text-[10px] uppercase tracking-wider text-text-muted">Examples</div>
					<pre class="text-xs text-accent leading-relaxed"><span class="text-text-muted"># Enable for auto-detected agent</span>
$ opensession ops stream enable

<span class="text-text-muted"># Enable for a specific agent</span>
$ opensession ops stream enable --agent claude-code

<span class="text-text-muted"># Disable streaming</span>
$ opensession ops stream disable</pre>
				</div>
			</div>

			<!-- index & completion -->
			<div id="cli-misc" class="docs-section border-t border-border pt-5">
				<h3 class="mb-1 text-sm font-bold text-text-primary">
					<code class="text-accent">opensession session index</code> /
					<code class="text-accent">completion</code>
				</h3>

				<div class="mt-3 border border-border bg-bg-secondary p-4">
					<div class="mb-2 text-[10px] uppercase tracking-wider text-text-muted">Examples</div>
					<pre class="text-xs text-accent leading-relaxed"><span class="text-text-muted"># Build/update the local session index</span>
$ opensession session index

<span class="text-text-muted"># Generate shell completions</span>
$ opensession docs completion bash >> ~/.bashrc
$ opensession docs completion zsh >> ~/.zshrc
$ opensession docs completion fish > ~/.config/fish/completions/opensession.fish</pre>
				</div>
			</div>

			<!-- Session References -->
			<div id="cli-refs" class="docs-section border-t border-border pt-5">
				<h3 class="mb-2 text-sm font-bold text-text-primary">Session References</h3>
				<p>
					The <code class="text-accent">handoff</code> and <code class="text-accent">diff</code> commands accept flexible session references:
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

<span class="text-text-muted"># Compare 2nd-most-recent vs most-recent</span>
$ opensession session diff HEAD^1 HEAD^0

<span class="text-text-muted"># Reference by ID prefix</span>
$ opensession session handoff abc12</pre>
				</div>
			</div>

			<!-- Output Formats -->
			<div id="cli-formats" class="docs-section border-t border-border pt-5">
				<h3 class="mb-2 text-sm font-bold text-text-primary">Output Formats</h3>
				<p>
					Available via <code class="text-accent">--format</code> across <code class="text-accent">log</code>,
					<code class="text-accent">handoff</code>, <code class="text-accent">stats</code>, and other commands:
				</p>

				<div class="mt-3 grid gap-px border border-border bg-border">
					<div class="flex bg-bg-secondary px-4 py-2 text-xs">
						<code class="w-24 shrink-0 font-bold text-accent">text</code>
						<span>Human-readable text (default for log, stats)</span>
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
      - OPENSESSION_BASE_URL=https://your-domain.com
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
					<code class="w-48 shrink-0 font-bold text-accent">OPENSESSION_BASE_URL</code>
					<span>Public-facing URL (default: http://localhost:3000)</span>
				</div>
				<div class="flex bg-bg-primary px-4 py-2 text-xs">
					<code class="w-48 shrink-0 font-bold text-accent">PORT</code>
					<span>HTTP listen port (default: 3000)</span>
				</div>
			</div>

			<h3 class="text-sm font-bold text-text-primary">Point the CLI to Your Instance</h3>
			<div class="border border-border bg-bg-secondary p-4">
				<code class="block text-xs text-accent">$ opensession account config --server http://localhost:3000</code>
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
