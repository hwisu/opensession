# Documentation

Everything you need to know about using OpenSession — uploading sessions, managing teams, and self-hosting your own instance.

## Getting Started

### Create an Account

Sign up at [/register](https://opensession.io/register) with a username and password, or use OAuth (GitHub, Google) if available on your instance.

### Sign In

Log in at [/login](https://opensession.io/login). Password and OAuth methods are both supported. After signing in, you'll land on your session list.

### Get Your API Key

Navigate to [/settings](https://opensession.io/settings) to find your API key (starts with `osk_`). You'll need this for uploading sessions via the CLI.

## Sessions

### Uploading Sessions

There are two ways to upload sessions:

**CLI Upload**

```bash
opensession publish upload ./session.jsonl
opensession publish upload-all     # discover and upload all local sessions
```

**Web Upload**

Drag and drop `.jsonl` files onto the [/upload](https://opensession.io/upload) page, or click to select files. You can optionally assign a team before uploading.

### Viewing Sessions

Your session list at [/](https://opensession.io/) shows all uploaded sessions. Each card displays the session tool, model, timestamp, token count, and a preview of the conversation.

### Timeline View

Click into any session to see the full timeline — every message, tool call, file edit, and error in chronological order. Use the sidebar to jump between events or filter by type.

### Search & Filter

Use the search bar (`/` to focus) to search across all sessions by content. Filter by tool, model, or team using the filter controls.

## Teams

### Creating a Team

Go to [/teams](https://opensession.io/teams) and click **Create Team**. Give your team a name — you'll be the admin automatically.

### Inviting Members

From the team detail page, invite members by their email address or OAuth username (e.g., GitHub username). If they already have an account, the invitation links to their existing account. Otherwise, they'll be prompted to register when accepting.

### Roles

- **admin** — Can invite/remove members, change roles, delete the team, and view all team sessions and stats.
- **member** — Can upload sessions to the team and view all team sessions and stats.

### Managing Members

Admins can remove members or change their role from the team detail page. Removing a member does not delete their previously uploaded sessions.

## Invitations

When you're invited to a team, a badge appears in the top navigation bar. Navigate to [/invitations](https://opensession.io/invitations) to see all pending invitations.

### Accepting

Click **Accept** to join the team. You'll immediately gain access to all team sessions and can start uploading sessions to the team.

### Declining

Click **Decline** to reject the invitation. The team admin can send a new invitation later if needed.

## Team Stats

Each team has a stats view showing usage across the team. Access it from the team detail page.

### By User

See how many sessions, tokens, and tool calls each team member has contributed.

### By Tool

Break down usage by AI tool — see which tools your team uses most (Claude Code, Cursor, Codex, etc.).

### Time Ranges

Filter stats by time range to see usage trends over the past week, month, or custom date ranges.

## CLI Reference

### Installation

```bash
cargo install opensession
```

Running `opensession` without arguments launches the TUI. Subcommands run CLI operations.

---

### `opensession account config`

Show or set CLI configuration.

**Flags:**

| Flag | Description |
|------|-------------|
| `--server <URL>` | Server URL |
| `--api-key <KEY>` | API key (`osk_` prefix) |
| `--team-id <ID>` | Default team ID |

**Examples:**

```bash
# Show current configuration
opensession account config

# Set server URL and API key
opensession account config --server https://opensession.io --api-key osk_abc123

# Set default team for uploads
opensession account config --team-id my-team
```

Config file: `~/.config/opensession/config.toml`

---

### `opensession session discover`

Scan this machine for AI sessions from all supported tools.

**Example:**

```bash
opensession session discover

# Output:
# Found 47 sessions:
#   claude-code  32 sessions  ~/.claude/projects/
#   cursor        8 sessions  ~/.cursor/
#   goose         4 sessions  ~/.config/goose/
#   aider         3 sessions  ~/.aider/
```

Supported: Claude Code, Cursor, Codex, Goose, Aider, OpenCode, Amp.

---

### `opensession publish upload` / `publish upload-all`

Upload session files to the server.

**Flags:**

| Flag | Description |
|------|-------------|
| `<file>` | Path to session file (required for `upload`) |
| `--parent <ID>` | Link to parent session(s), repeatable |
| `--git` | Store on git branch instead of server |

**Examples:**

```bash
# Upload a single session
opensession publish upload ./session.jsonl

# Upload with parent session linkage
opensession publish upload ./followup.jsonl --parent abc123

# Discover and upload all sessions at once
opensession publish upload-all

# Store session in git branch instead of server
opensession publish upload ./session.jsonl --git
```

`upload-all` skips subagent files and already-uploaded sessions automatically.

---

### `opensession session log`

Show session history in a git-log style format.

**Flags:**

| Flag | Description |
|------|-------------|
| `--since <TIME>` | Filter by time (e.g. "3 hours", "2 days", "1 week") |
| `--before <TIME>` | Show sessions before this time |
| `--tool <TOOL>` | Filter by tool (e.g. "claude-code", "cursor") |
| `--model <MODEL>` | Filter by model (supports wildcards: "opus*") |
| `--grep <QUERY>` | Search in titles and descriptions |
| `--touches <FILE>` | Show sessions that touched a specific file |
| `--has-errors` | Show only sessions with errors |
| `--project <PATH>` | Filter by working directory |
| `-n, --limit <N>` | Max results (default: 20) |
| `--format <FMT>` | Output format (text, json, jsonl, markdown) |
| `--json [FIELDS]` | Select JSON fields (e.g. "id,tool,title") |
| `--jq <FILTER>` | Apply jq filter to JSON output |

**Available JSON fields:** `id`, `tool`, `model`, `title`, `description`, `created_at`, `duration_seconds`, `message_count`, `event_count`, `total_input_tokens`, `total_output_tokens`, `has_errors`, `files_modified`, `working_directory`, `git_repo_name`, `source_path`, `git_remote`, `git_branch`, `git_commit`, `tags`

**Examples:**

```bash
# Show recent sessions
opensession session log

# Sessions from the last 3 hours
opensession session log --since "3 hours"

# Only Claude Code sessions with errors
opensession session log --tool claude-code --has-errors

# Search for sessions about authentication
opensession session log --grep "auth" --limit 5

# Sessions that touched a specific file
opensession session log --touches src/auth.rs

# Filter by model using wildcards
opensession session log --model "opus*"

# Export as JSON with specific fields
opensession session log --json "id,tool,title,created_at"

# Pipe through jq for custom queries
opensession session log --format json --jq '.[] | select(.has_errors)'
```

Auto-detection: when no explicit `--project` is specified, filters by the current git repo or working directory.

---

### `opensession session stats`

Show AI usage statistics — sessions, tokens, costs, and breakdowns by tool.

**Flags:**

| Flag | Description |
|------|-------------|
| `--period <PERIOD>` | Time period: day, week (default), month, all |
| `--format <FMT>` | Output format: text (default), json |

**Examples:**

```bash
# This week's stats
opensession session stats

# All-time usage
opensession session stats --period all

# Today's stats in JSON
opensession session stats --period day --format json
```

Shows: total sessions, duration, token counts (input/output), breakdown by tool, top edited files, error rate, and estimated cost.

---

### `opensession session handoff`

Generate a session summary for handing off context to the next AI agent.

**Flags:**

| Flag | Description |
|------|-------------|
| `[files...]` | Session file path(s). Multiple files merge into one handoff |
| `-l, --last` | Use the most recent session |
| `--claude <REF>` | Claude Code session reference (HEAD, HEAD~2) |
| `--gemini <REF>` | Gemini session reference |
| `--tool <TOOL_REF>` | Generic tool reference (e.g. "amp HEAD~2"), repeatable |
| `--summarize` | Generate LLM-powered summary |
| `--ai <PROVIDER>` | AI provider for summarization: claude, openai, gemini |
| `-o, --output <PATH>` | Write to file instead of stdout |
| `--format <FMT>` | Output format (default: markdown) |

**Examples:**

```bash
# Handoff from the last Claude Code session
opensession session handoff --claude HEAD

# Handoff with AI-powered summary
opensession session handoff --last --summarize

# Merge multiple sessions into one handoff
opensession session handoff session1.jsonl session2.jsonl

# Save handoff to a file
opensession session handoff --claude HEAD -o handoff.md

# Cross-tool handoff: Claude to Gemini
opensession session handoff --claude HEAD~3 --summarize --ai gemini
```

---

### `opensession session diff`

Compare two sessions side-by-side.

**Flags:**

| Flag | Description |
|------|-------------|
| `<session_a>` | First session (ID, file path, or reference) |
| `<session_b>` | Second session |
| `--ai` | Use AI to analyze differences |

**Examples:**

```bash
# Compare two sessions by file path
opensession session diff ./before.jsonl ./after.jsonl

# Compare using session references
opensession session diff HEAD^2 HEAD^1

# AI-powered diff analysis
opensession session diff HEAD^2 HEAD^1 --ai
```

---

### `opensession ops daemon`

Manage the background daemon that watches for new sessions and syncs them.

**Subcommands:**

| Subcommand | Description |
|------------|-------------|
| `start` | Start the background daemon |
| `stop` | Stop the daemon |
| `status` | Show daemon status |
| `health` | Check daemon and server health |

**Examples:**

```bash
# Start the daemon in the background
opensession ops daemon start

# Check if daemon is running
opensession ops daemon status

# Verify daemon + server connectivity
opensession ops daemon health

# Stop the daemon
opensession ops daemon stop
```

The daemon watches for new sessions from configured tools and syncs them to the server. Configure via `~/.config/opensession/daemon.toml` or the TUI settings:

```toml
[daemon]
publish_on = "manual"        # session_end | realtime | manual
debounce_secs = 5

[watchers]
claude_code = true
cursor = false
goose = true
aider = true

[privacy]
strip_paths = true
strip_env_vars = true
```

---

### `opensession account server`

Check server connection and authentication.

| Subcommand | Description |
|------------|-------------|
| `status` | Check server health and version |
| `verify` | Verify API key authentication |

```bash
# Check if server is reachable
opensession account server status

# Verify your API key works
opensession account server verify
```

---

### `opensession ops hooks`

Manage git hooks that link AI sessions to git commits.

| Subcommand | Description |
|------------|-------------|
| `install` | Install the prepare-commit-msg hook |
| `uninstall` | Remove the hook |

```bash
# Install in current repo
opensession ops hooks install

# Remove from current repo
opensession ops hooks uninstall
```

When installed, the hook appends AI session metadata (tool, model, prompt) to your commit messages automatically.

---

### `opensession ops stream`

Enable or disable real-time session streaming to the server.

```bash
# Enable for auto-detected agent
opensession ops stream enable

# Enable for a specific agent
opensession ops stream enable --agent claude-code

# Disable streaming
opensession ops stream disable
```

---

### `opensession session index` / `docs completion`

```bash
# Build/update the local session index
opensession session index

# Generate shell completions
opensession docs completion bash >> ~/.bashrc
opensession docs completion zsh >> ~/.zshrc
opensession docs completion fish > ~/.config/fish/completions/opensession.fish
```

---

### Session References

The `handoff` and `diff` commands accept flexible session references:

| Reference | Description |
|-----------|-------------|
| `HEAD` | Latest session |
| `HEAD~N` | Latest N sessions (merged) |
| `HEAD^N` | Nth most recent session (0-indexed) |
| `<id>` | Session ID (prefix matching supported) |
| `<path>` | Path to a session file |

```bash
# Last Claude Code session
opensession session handoff --claude HEAD

# Last 3 Claude Code sessions merged
opensession session handoff --claude HEAD~3

# Compare 2nd-most-recent vs most-recent
opensession session diff HEAD^1 HEAD^0

# Reference by ID prefix
opensession session handoff abc12
```

---

### Output Formats

Available via `--format` across `log`, `handoff`, `stats`, and other commands:

| Format | Description |
|--------|-------------|
| `text` | Human-readable text (default for log, stats) |
| `markdown` | Markdown format (default for handoff) |
| `json` | JSON format |
| `jsonl` | JSONL (one JSON object per line) |
| `hail` | HAIL session format |
| `stream` | NDJSON stream |

## Self-Hosting

### Quick Start

```bash
docker run -p 3000:3000 -v opensession-data:/data \
  -e JWT_SECRET=your-secret-here \
  ghcr.io/hwisu/opensession
```

This starts the server on port 3000 with an embedded SQLite database and persistent storage.

### Docker Compose

For production use:

```yaml
services:
  opensession:
    image: ghcr.io/hwisu/opensession
    ports:
      - "3000:3000"
    volumes:
      - opensession-data:/data
    environment:
      - JWT_SECRET=your-secret-here
      - BASE_URL=https://your-domain.com
    restart: unless-stopped

volumes:
  opensession-data:
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `JWT_SECRET` | *(required)* | Secret for JWT token signing |
| `OPENSESSION_DATA_DIR` | `data/` | SQLite DB and session body storage |
| `BASE_URL` | `http://localhost:3000` | Public-facing URL (used as OAuth callback base when set) |
| `PORT` | `3000` | HTTP listen port |

### Point the CLI to Your Instance

```bash
opensession account config --server http://localhost:3000
```

## HAIL Format

**HAIL** (Human-AI Interaction Log) is an open JSONL format for recording AI coding sessions. Each line is a self-contained JSON object representing one event.

### Structure

Every HAIL file starts with a metadata line, followed by events:

```jsonl
{"v":"hail/0.1","tool":"claude-code","model":"opus-4","ts":"2025-01-01T00:00:00Z"}
{"role":"human","content":"Fix the auth bug"}
{"role":"agent","content":"I'll update...","tool_calls":[...]}
{"type":"file_edit","path":"src/auth.rs","diff":"..."}
```

### Event Types

- **human** — User messages and prompts
- **agent** — AI responses and reasoning
- **tool_call** — Tool invocations and file edits
- **error** — Errors and failures

### Why JSONL?

Line-oriented JSON is streamable, appendable, and works with standard tools like `jq`, `grep`, and `wc -l`. No special parser needed — any language with JSON support can read HAIL files.
