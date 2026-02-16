# Parser Source Matrix

This document tracks parser reference sources and reuse boundaries for OpenSession.

## Scope

- Date baseline: 2026-02-16
- Target tools: Codex, Claude Code, Cursor, Gemini CLI, OpenCode
- Policy:
- Open-source projects can be referenced directly.
- Closed-source products use clean-room behavior/format analysis only.

## Tool Matrix

| Tool | Source status | Storage format(s) observed | Reuse boundary | Primary reference |
| --- | --- | --- | --- | --- |
| Codex CLI | Open source | JSONL (`session_meta`, `response_item`, `event_msg`) | Re-implement parser behavior, no direct copy into closed contexts | https://github.com/openai/codex |
| Claude Code | Open source | JSONL (`user`, `assistant`, `tool_use`, `tool_result`) | Re-implement mappings + fallback heuristics | https://github.com/anthropics/claude-code |
| Cursor | App not fully open source | SQLite `state.vscdb` (`cursorDiskKV`, `ItemTable`, `composerData:*`, `bubbleId:*`) | Behavior inference from local DB payloads and runtime artifacts only | https://github.com/cursor/cursor |
| Gemini CLI | Open source | JSON and JSONL sessions under `~/.gemini/tmp/*/chats` | Re-implement format adapters for schema drift | https://github.com/google-gemini/gemini-cli |
| OpenCode | Open source | Session/message/part JSON tree under `storage/` | Re-implement schema adapters and part matching | https://github.com/opencode-ai/opencode |

## Claude Code Verification Notes (2026-02-16)

- Local package checked: `Claude Code 2.1.42` (`/opt/homebrew/bin/claude`).
- Local session storage confirmed under `~/.claude/projects/**/*.jsonl` (including `subagents/agent-*.jsonl`).
- Real entry types observed: `user`, `assistant`, `system`, `progress`, `queue-operation`, `summary`, `file-history-snapshot`.
- Real assistant content observed with `tool_use` where `tool_use_id` may be null.

Parser alignment evidence:

- `tool_use` -> `tool_result` fallback pairing path is covered and tested for missing IDs.
- `subagent` metadata/path handling is covered in parser tests.
- Real-data ignored test passed locally:
  - `cargo test -p opensession-parsers -- --ignored` includes `test_parse_team_session_with_subagents`.

Primary references:

- https://github.com/anthropics/claude-code

## Cursor Verification Notes (2026-02-16)

- Local package checked: `cursor 2.4.37` (CLI reports commit `7b9c34466f5c119e93c3e654bb80fe9306b6cc70`).
- Local storage confirmed under:
  - `~/Library/Application Support/Cursor/User/workspaceStorage/*/state.vscdb`
- Current DB shape observed:
  - table `ItemTable` populated
  - key `composer.composerData` present
  - `cursorDiskKV` may be empty depending on version/profile

Parser alignment evidence:

- Parser supports both `cursorDiskKV` and `ItemTable` paths.
- Parser supports v3 bubble restore flow (`bubbleId:*`) and `toolFormerData` recovery.
- Real-data ignored tests passed locally:
  - `deserialize_all_conversations_from_real_db`
  - `parse_real_cursor_database`

Primary references:

- https://github.com/cursor/cursor
- https://raw.githubusercontent.com/cursor/cursor/main/README.md

## Gemini Verification Notes (2026-02-16)

- Local package checked: `gemini-cli 0.28.2` (`/opt/homebrew/bin/gemini`).
- Local session storage confirmed under `~/.gemini/tmp/*/chats/session-*.json`.
- Real message shapes observed:
  - `messages[*].type`: `user`, `gemini`, `info`, `error`
  - `messages[*].content`: both `string` and `array` variants
  - `toolCalls` present in `gemini` messages

Parser alignment evidence:

- Parser handles both string and part-array `content`.
- Parser handles tool calls/results with semantic attributes.
- Real-data ignored test passed locally:
  - `parse_real_gemini_session`

Primary references:

- https://github.com/google-gemini/gemini-cli

## Codex Verification Notes (2026-02-16)

- Local package checked: `codex-cli 0.101.0` (`/opt/homebrew/bin/codex`).
- Company schema verified against upstream tag `rust-v0.101.0`:
  - `ResponseItem` includes `web_search_call` with optional `action`.
  - `WebSearchAction` variants are `search`, `open_page`, `find_in_page`.
  - `EventMsg` includes `token_count`, `agent_reasoning`, `agent_reasoning_raw_content`, `item_completed`, `context_compacted`.
- Local Codex session samples (`~/.codex/sessions`, 200 files) confirm real emission of:
  - `response_item.payload.type = web_search_call`
  - `action.type` distribution: `search`, `open_page`, `find_in_page`
  - `event_msg.payload.type = token_count`, `agent_reasoning`, `context_compacted`, `item_completed`

Parser alignment applied:

- `web_search_call` now maps by action semantics:
  - `search` -> `EventType::WebSearch`
  - `open_page` / `find_in_page` -> `EventType::WebFetch`
  - missing URL edge-case fallback -> `ToolCall(web_search)` (no silent drop)
- `token_count` now supports both legacy flat fields and newer nested payload:
  - `info.last_token_usage.*`
  - `info.total_token_usage.*`
- `event_msg.agent_reasoning_raw_content` now normalizes to `EventType::Thinking`.
- `event_msg.context_compacted` is preserved as `Custom(context_compacted)`.
- `event_msg.item_completed` with `Plan` item is preserved as `Custom(plan_completed)`.
- Conformance fixture added for `web_search_call` variants + nested token usage.

Primary references:

- https://github.com/openai/codex/tree/rust-v0.101.0/codex-rs
- https://github.com/openai/codex/blob/rust-v0.101.0/codex-rs/app-server-protocol/schema/typescript/ResponseItem.ts
- https://github.com/openai/codex/blob/rust-v0.101.0/codex-rs/app-server-protocol/schema/typescript/WebSearchAction.ts
- https://github.com/openai/codex/blob/rust-v0.101.0/codex-rs/app-server-protocol/schema/typescript/EventMsg.ts

## OpenCode Verification Notes (2026-02-16)

- Local package checked: `opencode 1.2.0` (`/opt/homebrew/bin/opencode`).
- Company-exported schema confirmed via `opencode export <sessionID>`:
  - top-level: `info`, `messages[*].info`, `messages[*].parts`.
  - message model fields appear both as top-level (`providerID`/`modelID`) and nested (`model.providerID`/`model.modelID`).
- Local storage schema confirmed under `~/.local/share/opencode/storage/`:
  - `session/<project>/<session_id>.json`
  - `message/<session_id>/<message_id>.json`
  - `part/<message_id>/<part_id>.json`
- Part types observed in real data: `text`, `reasoning`, `tool`, `step-start`, `step-finish`, `patch`, `file`.
- Tool status values observed in real data: `completed`, `error`, `running`.

Parser alignment applied:

- `reasoning` part now maps to `EventType::Thinking` (with encrypted-reasoning placeholder).
- `callID` is trimmed/normalized and reused for semantic pairing (`semantic.call_id`, `ToolResult.call_id`).
- Tool result emission handles status case drift (e.g. `Completed`) and metadata output fallback.
- `file` part now maps to human-readable message events (`Attached file: ...`).
- `patch` part now maps to `FileEdit` events per changed file.
- Conformance fixture added to lock these behaviors.

## Canonical Parser Rules

1. Source adapter:
- Detect schema/version per file.
- Persist source metadata in event attributes:
- `source.schema_version`
- `source.raw_type`

2. Semantic normalization:
- Normalize to HAIL `EventType`.
- Attach semantic metadata when available:
- `semantic.group_id`
- `semantic.call_id`
- `semantic.tool_kind`

3. Tool lifecycle:
- Emit `ToolCall` and `ToolResult` with stable pairing keys.
- Add fallback matching heuristics for missing IDs.

4. Content normalization:
- Keep text/code/json in structured blocks.
- Preserve line-numbered code mapping where possible.

5. Task boundary balancing:
- Close hanging tasks at EOF with synthetic `TaskEnd`.

## Verification Gates

- Parser unit/integration: `cargo test -p opensession-parsers`
- Hook parity:
- pre-commit: `cargo fmt --all -- --check` + `cargo test -p opensession-daemon --quiet`
- pre-push: clippy/wasm/web checks + docker e2e
