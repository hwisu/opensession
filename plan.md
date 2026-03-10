# ACP Semantic JSONL을 OpenSession Canonical Format으로 전환

## Summary

- HAIL v1 `header/event/stats`를 더 확장하지 않고, OpenSession의 canonical 저장 포맷을 `ACP semantic JSONL`로 바꾼다.
- 저장은 `full wire capture`가 아니라 `ACP 의미 단위`를 한 줄씩 기록하는 JSONL로 고정한다.
- 목표는 “ACP와 거의 비슷한 포맷”이 아니라 “OpenSession이 ACP-native처럼 읽히고 replay 가능한 저장 포맷”이다.
- OpenSession 고유 값은 루트 확장이 아니라 `_meta.opensession.*`로만 저장한다.
- 레거시 HAIL v1은 읽기 호환만 유지하고, 새 쓰기 경로는 전부 ACP semantic JSONL만 쓴다.

## Key Changes

### 1. Canonical format을 ACP semantic JSONL로 재정의

- canonical line type은 아래 3개로 고정한다.
  - `session.new`
  - `session.update`
  - `session.end`
- 각 line은 JSONL 한 줄에 하나씩 기록한다.
- `session.new`는 세션 생성 정보와 replay 시작점만 담는다.
  - `sessionId`
  - `cwd` when present
  - `mcpServers` when known
  - `_meta.opensession.agent`
  - `_meta.opensession.context`
- `session.update`는 기존 HAIL `Event`를 ACP update 단위로 변환해 담는다.
- `session.end`는 종료 상태와 집계 정보를 담는다.
  - ACP core 종료 정보
  - `_meta.opensession.stats`
- standalone `stats` line은 더 이상 쓰지 않는다.

### 2. Event mapping을 ACP 기준으로 고정

- `UserMessage`, `AgentMessage`, `SystemMessage`, `Thinking`은 ACP session update 계열로 변환한다.
- `ToolCall`은 ACP `tool_call`로 변환한다.
- `ToolResult`는 ACP `tool_call_update`로 변환한다.
- `toolCallId`는 기존 `call_id`가 있으면 그대로 쓰고, 없으면 stable synthetic id를 `event_id` 기반으로 생성한다.
- `FileRead`, `FileEdit`, `ShellCommand`, `WebSearch`, `WebFetch`, `CodeSearch`, `FileSearch` 같은 semantic action은 ACP tool call/tool update로 정규화한다.
- `FileEdit.diff`는 ACP가 표현 가능한 diff content로 매핑하고, 부족한 필드는 `_meta.opensession.diff`로 보존한다.
- ACP core에 없는 값은 새 루트 필드를 만들지 않고 `_meta.opensession.event`에 넣는다.
  - `eventId`
  - `taskId`
  - `durationMs`
  - `originalEventType`
  - `originalContent`
  - source parser raw attrs

### 3. OpenSession extension 규칙을 `_meta`로 통일

- OpenSession 고유 정보는 모두 `_meta.opensession.*` 아래로 이동한다.
- v1 extension namespace는 아래로 고정한다.
  - `_meta.opensession.agent`
  - `_meta.opensession.context`
  - `_meta.opensession.job`
  - `_meta.opensession.stats`
  - `_meta.opensession.review`
  - `_meta.opensession.handoff`
  - `_meta.opensession.source`
  - `_meta.opensession.event`
- 기존 `opensession.job.*`는 내부 `Session` 모델과 DB projection에서는 유지하되, ACP semantic JSONL persistence에서는 `_meta.opensession.job`의 structured object로 저장한다.
- trace/correlation 값은 ACP 권장대로 `_meta.traceparent`, `_meta.tracestate`, `_meta.baggage`를 그대로 허용한다.

### 4. Core read/write를 single-write, dual-read로 전환

- `Session::to_jsonl()`은 ACP semantic JSONL만 출력한다.
- `Session::from_jsonl()`은 아래 두 포맷을 auto-detect 한다.
  - legacy HAIL v1 `header/event/stats`
  - ACP semantic JSONL
- explicit helper를 유지한다.
  - `to_hail_v1_jsonl()`
  - `from_hail_v1_jsonl()`
  - `to_acp_semantic_jsonl()`
  - `from_acp_semantic_jsonl()`
- parser는 계속 내부 `Session`을 만들고, writer에서 ACP semantic JSONL로 직렬화한다.
- capture/register/review/handoff는 내부 `Session` API를 계속 써서 저장 포맷 변경의 충격을 제한한다.

### 5. API/CLI/UI surface는 body format만 바꾸고 의미는 유지

- `/api/sessions/:id/raw`는 ACP semantic JSONL body를 반환한다.
- `SessionSummary`, `SessionDetail`, `job_context`, `JobReviewBundle`은 기존 shape를 유지한다.
- `opensession capture import`는 입력 계약은 유지하고, 출력 body만 ACP semantic JSONL로 쓴다.
- desktop/web/local review/change-reader/summary는 raw body를 직접 HAIL v1 구조로 가정하지 않고, `Session::from_jsonl()` 또는 ACP-aware parser를 통해 읽는다.
- worker에도 `/api/review/job/:job_id`를 제공해서 same-origin job review 경로를 닫는다.

## Public APIs / Interfaces

- Canonical persisted format
  - ACP semantic JSONL
  - line kinds: `session.new`, `session.update`, `session.end`
- Core helpers
  - `Session::to_jsonl()` -> ACP semantic JSONL write
  - `Session::from_jsonl()` -> legacy HAIL v1 + ACP semantic auto-detect read
  - explicit legacy/ACP helpers 유지
- Extension policy
  - custom data only in `_meta`
  - OpenSession namespace is `_meta.opensession.*`
- Existing typed APIs remain
  - `SessionSummary.job_context`
  - `SessionDetail.job_context`
  - `GET /api/review/job/:job_id?kind=todo|done&run_id=<optional>`
- Same-origin worker review API
  - `GET /api/review/job/:job_id?kind=todo|done&run_id=<optional>`

## Test Plan

- Core format tests
  - ACP semantic JSONL write snapshot
  - ACP semantic JSONL read snapshot
  - legacy HAIL v1 auto-detect read
  - legacy HAIL v1 -> normalized `Session` -> ACP semantic JSONL rewrite
- Mapping tests
  - message event mapping
  - tool call / tool call update mapping
  - stable `toolCallId` generation and reuse
  - diff/location/rawInput/rawOutput preservation
  - `_meta.opensession.*` lossless roundtrip
- Existing product behavior regression
  - `capture import`
  - local DB indexing and `job_context`
  - `/api/sessions`
  - `/api/review/job`
  - handoff build/read
  - summary/change-reader prompts still operate on normalized `Session`
- Web/Desktop validation
  - `/sessions` still renders ACP-backed sessions
  - `/review/job/[job_id]` works from worker/server path
  - desktop raw-body/session-detail flows continue to parse the new body format

## Assumptions

- canonical write format은 ACP semantic JSONL이고, full JSON-RPC wire capture는 채택하지 않는다.
- OpenSession은 ACP core를 우선 따르고, 부족한 값만 `_meta.opensession.*`에 넣는다.
- legacy HAIL v1은 migration 기간 동안 읽기 전용으로 유지한다.
- parser 내부 모델을 한 번에 ACP object graph로 바꾸지 않고, 우선 `Session` normalization을 유지한 채 persistence format부터 ACP로 바꾼다.
- 최대 호환성을 위해 새 루트 필드는 만들지 않고, ACP core + `_meta` 원칙만 사용한다.
