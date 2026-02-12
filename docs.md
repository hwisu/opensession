# Documentation

Everything you need to know about using OpenSession — uploading sessions, managing teams, and self-hosting your own instance.

## Getting Started

### Create an Account

Sign up at [/register](https://opensession.io/register) with a username and password, or use OAuth (GitHub, Google) if available on your instance.

### Sign In

Log in at [/login](https://opensession.io/login). Password and OAuth methods are both supported. After signing in, you'll land on your session list.

### Get Your API Key

Navigate to [/settings](https://opensession.io/settings) to find your API key. You'll need this for uploading sessions via the CLI.

## Sessions

### Uploading Sessions

There are two ways to upload sessions:

CLI Upload

`$ opensession upload ./session.jsonl` `$ opensession upload-all ~/.claude/projects/`

Web Upload

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

From the team detail page, invite members by their email address. If they already have an account (including OAuth accounts), the invitation links to their existing account. Otherwise, they'll be prompted to register when accepting.

### Roles

admin

Can invite/remove members, change roles, delete the team, and view all team sessions and stats.

member

Can upload sessions to the team and view all team sessions and stats.

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

`$ cargo install opensession`

### Configuration

Set your API endpoint and key:

`$ opensession config set api-url https://opensession.io` `$ opensession config set api-key YOUR_API_KEY`

### Commands

`opensession upload <file>`

Upload a single HAIL session file. Supports `.jsonl` files.

`opensession upload-all <directory>`

Recursively scan a directory and upload all HAIL session files found. Skips already-uploaded sessions.

`opensession config set <key> <value>`

Set a configuration value. Keys: `api-url`, `api-key`.

## Self-Hosting

### Quick Start

Run OpenSession with a single command:

`$ docker run -p 3000:3000 ghcr.io/hwisu/opensession`

This starts the server on port 3000 with an embedded SQLite database. Data persists inside the container — mount a volume for durability.

### Docker Compose

For production use with persistent storage:

services:
  opensession:
    image: ghcr.io/hwisu/opensession
    ports:
      - "3000:3000"
    volumes:
      - opensession-data:/data
    environment:
      - DATABASE\_URL=/data/opensession.db

volumes:
  opensession-data:

### Point the CLI to Your Instance

`$ opensession config set api-url http://localhost:3000`

## HAIL Format

**HAIL** (Human-AI Interaction Log) is an open JSONL format for recording AI coding sessions. Each line is a self-contained JSON object representing one event.

### Structure

Every HAIL file starts with a metadata line, followed by events:

```
// Line 1: Session metadata
{"v":"hail/0.1","tool":"claude-code","model":"opus-4","ts":"..."}

// Subsequent lines: Events
{"role":"human","content":"Fix the auth bug"}
{"role":"agent","content":"I'll update...","tool_calls":[...]}
{"type":"file_edit","path":"src/auth.rs","diff":"..."}
```

### Event Types

human

User messages and prompts

agent

AI responses and reasoning

tool\_call

Tool invocations and file edits

error

Errors and failures

### Why JSONL?

Line-oriented JSON is streamable, appendable, and works with standard tools like `jq`, `grep`, and `wc -l`. No special parser needed — any language with JSON support can read HAIL files.
