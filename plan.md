# OpenSession x Symphony

## 목표

- OpenSession은 `Symphony runner`를 대체하지 않는다.
- 역할은 `에이전트 로컬 실행기록 + job/review metadata`를 받아 `히스토리 조회`, `todo review`, `job done review`, `handoff`를 쉽게 여는 `local review ledger`로 고정한다.
- v1 기본 경로는 `설치 + 추출(call)`이다. Symphony는 raw event를 실시간 push하지 않고, 각 체크포인트에서 OpenSession CLI를 호출해 로컬 로그를 정규화/등록한다.

## 기존 형식 vs 새 형식

| 항목 | 기존 | 새 확장 |
| --- | --- | --- |
| 중심 단위 | `session` | `session` 그대로 + `job context` |
| 리뷰 단위 | PR/commit range | PR/commit 유지 + `job_id` 기반 `todo/done review` |
| 조회 키 | search/tool/repo/time | 기존 키 + `protocol/job_id/run_id/stage/review_kind/status` |
| 외부 연동 | OpenSession 내부 관례 필요 | Symphony가 stable manifest와 CLI call만 알면 됨 |
| 서버 의존성 | 선택적 | v1은 서버 불필요, 로컬 설치만 필수 |

## 프로토콜 영향

| 출처 | OpenSession에 주는 영향 |
| --- | --- |
| Agent Client Protocol | 세션 한 개의 표현 방식. Markdown/diff/agent-editor-neutral session rendering의 기준 |
| Agent Communication Protocol | 여러 세션을 job/run/review 단위로 묶는 방식. `job_id`, `artifacts`, `async task history`, `discovery` vocabulary의 기준 |

## 핵심 설계

### 1. Canonical job metadata

`SessionContext.attributes`에는 아래 canonical key를 사용한다.

- `opensession.job.protocol`
- `opensession.job.system`
- `opensession.job.id`
- `opensession.job.title`
- `opensession.job.run_id`
- `opensession.job.attempt`
- `opensession.job.stage`
- `opensession.job.review_kind`
- `opensession.job.status`
- `opensession.job.thread_id`
- `opensession.job.artifacts`

v1 enum 값은 아래로 고정한다.

- `protocol`: `opensession | agent_client_protocol | agent_communication_protocol`
- `stage`: `planning | review | execution | handoff`
- `review_kind`: `todo | done`
- `status`: `pending | in_progress | completed | failed | cancelled`

### 2. Symphony가 call하는 glue layer

Symphony는 OpenSession 내부 DB나 HTTP 구조를 몰라도 된다. v1 통합 계약은 아래 두 파일뿐이다.

- native log file path
- job manifest file path

호출 명령은 아래로 고정한다.

```bash
opensession capture import \
  --profile codex \
  --log /path/to/native-log.jsonl \
  --manifest /path/to/job_manifest.json
```

이 명령의 책임은 아래와 같다.

- native log 읽기
- 기존 parser로 HAIL 변환
- manifest를 session attributes에 병합
- canonical validation
- local object store 등록
- local DB index upsert
- `session_id`, `local_uri`, `job_id`, `run_id`, `review_url` 출력

### 3. 로컬 설치-추출을 기본값으로 유지

v1 운영 모델은 아래와 같다.

1. runner machine에 `opensession` 설치
2. Symphony가 각 checkpoint마다 local CLI 호출
3. OpenSession이 local object store + local DB + local review UI 제공

v1에서는 아래를 만들지 않는다.

- raw event streaming API
- central server-first ingest
- Symphony 전용 dashboard

### 4. OpenSession 조회 표면

세션 목록/상세 응답은 raw attributes 대신 `job_context` typed block을 제공한다.

- `protocol`
- `system`
- `job_id`
- `job_title`
- `run_id`
- `attempt`
- `stage`
- `review_kind`
- `status`
- `thread_id`
- `artifact_count`

세션 목록 필터는 아래를 추가한다.

- `protocol`
- `job_id`
- `run_id`
- `stage`
- `review_kind`
- `status`

### 5. Job review bundle

새 읽기 엔드포인트는 아래다.

- `GET /api/review/job/:job_id?kind=todo|done`
- `run_id`는 optional이고, 없으면 해당 review kind의 최신 run을 선택한다.

`todo review`의 히스토리 규칙:

- selected run의 `planning`
- selected run의 `review(todo)`

`done review`의 히스토리 규칙:

- selected run의 `review(todo)`
- selected run의 `execution`
- selected run의 `review(done)`
- selected run의 `handoff`

### 6. 웹 표면

새 웹 라우트:

- `/review/job/[job_id]?kind=todo|done`
- `run_id` optional

핵심 블록:

- job header
- review kind badge
- run selector
- session timeline
- reviewer quick digest
- semantic summary
- artifacts
- handoff panel

## Manifest 계약

최소 스키마:

```json
{
  "protocol": "agent_communication_protocol",
  "system": "symphony",
  "job_id": "AUTH-123",
  "job_title": "Fix auth bug",
  "run_id": "run-42",
  "attempt": 1,
  "stage": "review",
  "review_kind": "todo",
  "status": "pending",
  "thread_id": "thread-9",
  "artifacts": [
    {
      "kind": "plan",
      "label": "Plan note",
      "uri": "file:///tmp/plan.md"
    }
  ]
}
```

artifact 항목은 아래 필드를 사용한다.

- `kind`
- `label`
- `uri`
- `mime_type` optional
- `metadata` optional

## Symphony 호출 시점

v1 권장 호출 시점은 아래 네 가지다.

1. `todo review` 직전
2. execution 종료 직후
3. `done review` 직전 또는 직후
4. handoff 생성 직후

예시:

```bash
opensession capture import \
  --profile codex \
  --log ./.codex/sessions/rollout.jsonl \
  --manifest ./job_manifest.todo.json
```

```bash
opensession capture import \
  --profile codex \
  --log ./.codex/sessions/rollout.jsonl \
  --manifest ./job_manifest.done.json
```

review URL 예시:

```text
http://127.0.0.1:8788/review/job/AUTH-123?kind=todo&run_id=run-42
```

## 구현 우선순위

1. canonical job metadata
2. `opensession capture import`
3. local DB projection + session API `job_context`
4. `GET /api/review/job/:job_id`
5. `/review/job/[job_id]` 웹 라우트
6. 세션 목록 필터 확장
7. 선택적 공유/upload는 v2로 연기

## 검증 계획

- CLI
  - `capture import` 성공 경로
  - malformed manifest 에러 경로
  - `--no-register` 경로
- parser/glue
  - 기존 parser fidelity 유지
  - manifest merge 후 canonical validation 유지
- DB/API
  - `job_context` projection
  - `protocol/job_id/run_id/stage/review_kind/status` 필터
- review bundle
  - latest run fallback
  - explicit `run_id` 선택
  - `todo review`/`done review` stage selection
- web
  - `/sessions` job filter
  - `/review/job/:job_id?kind=todo`
  - `/review/job/:job_id?kind=done`

## 비목표

- Symphony orchestration 재구현
- agent event streaming ingestion
- central review server를 v1의 필수 요소로 만드는 것
- Symphony 전용 독립 UI
