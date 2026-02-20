# opensession

[![English](https://img.shields.io/badge/lang-English-blue)](README.md)
[![CI](https://github.com/hwisu/opensession/actions/workflows/ci.yml/badge.svg)](https://github.com/hwisu/opensession/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/opensession.svg)](https://crates.io/crates/opensession)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

오픈소스 AI 코딩 세션 매니저입니다. Claude Code, Cursor, Codex, Goose, Aider 등
다양한 AI 도구의 세션을 수집, 탐색, 공유할 수 있습니다.

**웹사이트**: [opensession.io](https://opensession.io)  
**GitHub**: [github.com/hwisu/opensession](https://github.com/hwisu/opensession)

## 현재 방향

OpenSession은 git-native 워크플로를 기본으로 전환했습니다.
- Docker 필수 운영 흐름을 제거했습니다.
- 서버 프로필: 인증 + 세션 조회/업로드.
- Worker 프로필: 공개 세션 조회 전용(read-only).
- 팀/초대/싱크 라우트는 활성 런타임 경로에서 정리되었습니다.

## 빠른 시작

### CLI

```bash
cargo install opensession

opensession --help
opensession session handoff --last
opensession daemon start --repo .
opensession daemon enable-hook --agent claude-code
```

수동 로컬 탐색 모드(TUI):

```bash
opensession      # 전체 로컬 세션
opensession .    # 현재 git 레포 범위
```

선택적 시작 동작:

```bash
OPS_TUI_REFRESH_DISCOVERY_ON_START=0 opensession
```

`0|false|off|no`로 설정하면 TUI 시작 시 전체 디스크 재탐색을 건너뛰고, 로컬 DB 캐시 세션을 우선 사용합니다.

TUI/Web Share 인증은 개인 API 키 기준입니다(TUI 내부 이메일 로그인 경로 없음):
```bash
opensession account connect --server https://opensession.io --api-key <issued_key>
```
키는 웹 설정 페이지(`/settings`)에서 발급하며, 발급 응답에서 1회만 표시됩니다.

## 런타임 기능

| 항목 | Server (Axum) | Worker (Wrangler) |
|------|----------------|-------------------|
| 홈(`/`) | 게스트 랜딩, 로그인 후 세션 목록 | 게스트 랜딩, 로그인 후 세션 목록 |
| 업로드 UI(`/upload`) | 사용 가능 | 비활성(read-only) |
| API 표면 | `/api/health`, `/api/capabilities`, `/api/sessions*`, `/api/auth*` | `/api/health`, `/api/capabilities`, `/api/sessions*`, `/api/auth*` |
| 인증 라우트 | `JWT_SECRET` 설정 시 활성 | `JWT_SECRET` 설정 시 활성 |
| 팀/초대/싱크 라우트 | 비활성 | 비활성 |

웹 UI 동작은 `GET /api/capabilities` 기반 런타임 감지로 결정됩니다(빌드 타임 프로필 플래그 없음).

## 아키텍처

```
┌─────────┐    ┌────────┐    ┌──────────────────┐
│  CLI /  │───▶│ daemon │───▶│ server (Axum)    │
│  TUI    │    │ (watch │    │ SQLite + disk     │
└─────────┘    │ +upload)│   │ :3000             │
               └────────┘    └──────────────────┘
```

단일 Cargo 워크스페이스, 12개 크레이트:

| 크레이트 | 설명 |
|---------|------|
| `core` | HAIL 도메인 모델 (타입/검증) |
| `parsers` | AI 도구 세션 파서 |
| `api` | 공용 API 타입, SQL 빌더, 서비스 로직 |
| `api-client` | 서버 통신용 HTTP 클라이언트 |
| `local-db` | 로컬 SQLite 인덱스/캐시 레이어(메타데이터, sync 상태, HEAD 참조) |
| `git-native` | `gix` 기반 Git 연산 |
| `server` | Axum HTTP 서버 + SQLite 저장소 |
| `daemon` | 백그라운드 감시/업로드 에이전트 |
| `cli` | CLI 엔트리 (`opensession`) |
| `tui` | 터미널 세션 탐색 UI |
| `worker` | Cloudflare Workers 백엔드 (WASM, 워크스페이스 제외) |
| `e2e` | E2E 테스트 |

## CLI 명령어

| 명령어 | 설명 |
|--------|------|
| `opensession` / `opensession .` | 로컬 인터랙티브 모드 실행 |
| `opensession session handoff` | 즉시 v2 실행 계약 핸드오프 생성 (`--validate`, `--strict`) |
| `opensession session handoff save ...` | 병합 handoff를 Git ref 정본 아티팩트로 저장 (`refs/opensession/handoff/artifacts/<id>`) |
| `opensession session handoff artifact ...` | 아티팩트 수명주기 (`list`, `show`, `refresh`, `render-md`) |
| `opensession publish upload <file> [--git]` | 단일 세션 퍼블리시 (기본: 서버, `--git`: `opensession/sessions` 브랜치) |
| `opensession daemon start\|stop\|status\|health` | 데몬 실행/중지/상태 |
| `opensession daemon enable-hook --agent <name>` | 스트림 훅 수동 설치 |
| `opensession daemon select --repo ...` | 감시 경로/레포 선택 |
| `opensession daemon show` | 현재 감시 대상 확인 |
| `opensession account connect` | 서버 URL/API 키 설정(선택) |
| `opensession account status\|verify` | 서버 연결 상태 확인 |
| `opensession docs completion <shell>` | 쉘 자동완성 생성 |

## Handoff 사용법 (실행 검증 완료)

이 레포에서 아래 명령을 실제로 실행해 확인했습니다.

```bash
# handoff 도움말
cargo run -p opensession -- session handoff --help

# v2 JSON + validation 리포트 (소프트 게이트, exit 0)
cargo run -p opensession -- session handoff --last --format json --validate

# strict validation 게이트 (error 위반 시 non-zero)
cargo run -p opensession -- session handoff --last --validate --strict

# 머신 소비용 stream envelope
cargo run -p opensession -- session handoff --last --format stream --validate

# 최근 N개 세션 (숫자 또는 HEAD~N)
cargo run -p opensession -- session handoff --last 6 --format json
cargo run -p opensession -- session handoff --last HEAD~6 --format json

# provider CLI로 HANDOFF.md 보강 생성
cargo run -p opensession -- session handoff --last 6 --populate claude
cargo run -p opensession -- session handoff --last 6 --populate claude:opus-4.6

# 정본 handoff artifact 저장/조회/갱신
cargo run -p opensession -- session handoff save --last 6 --payload-format jsonl
cargo run -p opensession -- session handoff artifact list
cargo run -p opensession -- session handoff artifact show <artifact_id>
cargo run -p opensession -- session handoff artifact refresh <artifact_id>
cargo run -p opensession -- session handoff artifact render-md <artifact_id> --output HANDOFF.md
```

반복 실행 성능이 중요하면 `cargo run` 대신 빌드된 바이너리를 사용하세요:
```bash
cargo build -p opensession
target/debug/opensession session handoff --last HEAD~2 --format json
```

CLI 종류별 예시(세션 생성)와 대응 handoff 명령:

| 소스 CLI | 예시 명령 | handoff 명령 |
|---|---|---|
| Claude Code | `claude -c` 또는 `claude -p "실패 테스트를 고치고 회귀 테스트를 추가해줘"` | `cargo run -p opensession -- session handoff --claude HEAD --validate` |
| Codex CLI | `codex exec "실패 테스트를 고치고 회귀 테스트를 추가해줘"` | `cargo run -p opensession -- session handoff --tool "codex HEAD" --validate` |
| OpenCode | `opencode run "실패 테스트를 고치고 회귀 테스트를 추가해줘"` | `cargo run -p opensession -- session handoff --tool "opencode HEAD" --validate` |
| Gemini CLI | `gemini -p "실패 테스트를 고치고 회귀 테스트를 추가해줘"` | `cargo run -p opensession -- session handoff --gemini HEAD --validate` |
| Amp CLI | `amp -x "실패 테스트를 고치고 회귀 테스트를 추가해줘"` | `cargo run -p opensession -- session handoff --tool "amp HEAD" --validate` |

참고:
- 최신 세션이 아니라 이전 세션이면 `HEAD` 대신 `HEAD~N`을 사용하세요.
- 전용 플래그가 없는 도구군은 `--tool "<name> <ref>"` 형식을 사용합니다.

동작 요약:
- `--validate`: 사람이 읽는 리포트 + JSON 리포트를 출력하고 종료코드 `0`.
- `--validate --strict`: error 레벨 위반이 있을 때만 non-zero 종료.
- 기본 스키마는 v2 실행 계약 출력입니다.
- handoff 정본은 Git ref 아티팩트(`refs/opensession/handoff/artifacts/<artifact_id>`)입니다.
- `HANDOFF.md`는 파생 산출물(`artifact render-md`)입니다.
- 아티팩트 병합 규칙은 시간순 오름차순(`time_asc`)으로 고정됩니다.
- 소스가 변경되면 stale로 표시되고, 자동 갱신 없이 `artifact refresh`로 수동 갱신합니다.
- `--populate <provider[:model]>`: handoff JSON을 provider CLI(`claude`, `codex`, `opencode`, `gemini`, `amp`) 표준입력으로 전달해 `HANDOFF.md` 초안을 생성합니다.
- `execution_contract.parallel_actions`: 순차 액션과 별도로 병렬 가능한 작업 패키지를 분리해 제공합니다.
- `execution_contract.ordered_steps`: 작업 순서/타임스탬프/의존성을 함께 보존해 시간성과 정합성을 잃지 않고 후속 에이전트가 재실행할 수 있게 합니다.

## Worker 로컬 개발 (Wrangler, 실행 검증 완료)

```bash
wrangler --version
wrangler dev --help

# 기본 로컬 실행
wrangler dev --ip 127.0.0.1 --port 8788

# 로컬 D1/R2 상태 유지
wrangler dev --ip 127.0.0.1 --port 8788 --persist-to .wrangler/state

# Cloudflare 엣지 원격 실행
wrangler dev --remote

# 디버그 로그
wrangler dev --ip 127.0.0.1 --port 8788 --log-level debug
```

메모:
- `wrangler dev`는 이 레포의 `sh build.sh`를 호출해 Worker를 로컬 서빙합니다.
- `wrangler.toml` 기준으로 D1/R2/assets/env 바인딩이 로컬에 연결됩니다.
- `--remote`는 Cloudflare 로그인/권한이 필요하고 실제 원격 리소스에 접근할 수 있습니다.

## 설정

데몬 훅 정책:
- `opensession daemon start`는 자동으로 훅을 설치하지 않습니다.
- 필요 시 명시적으로 설치합니다: `opensession daemon enable-hook --agent claude-code`.

표준 설정 파일:
- `~/.config/opensession/opensession.toml`

로컬 캐시 DB:
- `~/.local/share/opensession/local.db`
- 세션 본문의 정본 저장소가 아니라 로컬 인덱스/캐시(메타데이터, sync 상태, 타임라인 캐시)로 사용됩니다.

## local-db 범주

- `local-db`는 로컬 인덱스/캐시 용도로 사용됩니다:
  - `log`, `stats`, `HEAD~N` 해석
  - sync 상태 및 TUI 캐시 초기 로드
- 기본 운영 경로:
  - v2 handoff 스키마 + git-native 워크플로

예시:

```toml
[server]
url = "http://localhost:3000"
api_key = ""

[identity]
nickname = "user"

[watchers]
custom_paths = [
  "~/.claude/projects",
  "~/.codex/sessions",
]
```

## API 엔드포인트

| 메서드 | 경로 | 설명 |
|--------|------|------|
| GET | `/api/health` | 헬스 체크 |
| GET | `/api/capabilities` | 런타임 기능 플래그(`auth_enabled`, `upload_enabled`) |
| GET | `/api/auth/providers` | 사용 가능한 인증 공급자 목록 |
| POST | `/api/auth/register` | 이메일/비밀번호 회원가입 |
| POST | `/api/auth/login` | 이메일/비밀번호 로그인 |
| POST | `/api/auth/refresh` | 액세스 토큰 갱신 |
| POST | `/api/auth/logout` | 리프레시 토큰 무효화 |
| POST | `/api/auth/verify` | 액세스 토큰 검증 |
| GET | `/api/auth/me` | 현재 사용자 프로필(API 키 미포함) |
| POST | `/api/auth/api-keys/issue` | 개인 API 키 신규 발급(응답에서 1회만 노출) |
| POST | `/api/sessions` | HAIL 세션 업로드 (인증 필요) |
| GET | `/api/sessions` | 세션 목록 조회 |
| GET | `/api/sessions/{id}` | 세션 상세 조회 |
| GET | `/api/sessions/{id}/raw` | 원본 HAIL JSONL 다운로드 |
| DELETE | `/api/sessions/{id}` | 세션 삭제 |

API 키는 발급 응답에서만 확인할 수 있으며 `GET /api/auth/me`로는 다시 조회되지 않습니다.

## 셀프 호스팅 서버

```bash
cargo run -p opensession-server
# -> http://localhost:3000
```

주요 환경 변수:

| 변수 | 기본값 | 설명 |
|------|--------|------|
| `OPENSESSION_DATA_DIR` | `data/` | 서버 SQLite DB 및 blob 저장 경로 |
| `OPENSESSION_WEB_DIR` | `web/build` | 정적 프론트엔드 경로 |
| `OPENSESSION_PUBLIC_FEED_ENABLED` | `true` | `false`이면 익명 `GET /api/sessions` 차단 |
| `OPENSESSION_SESSION_SCORE_PLUGIN` | `heuristic_v1` | 세션 점수 플러그인 (`heuristic_v1`, `zero_v1`, custom) |
| `PORT` | `3000` | HTTP 리슨 포트 |

## 마이그레이션 정합성

원격 마이그레이션 파일은 다음 두 경로가 byte-identical 이어야 합니다.
- `migrations/*.sql`
- `crates/api/migrations/[0-9][0-9][0-9][0-9]_*.sql`

검증:

```bash
scripts/check-migration-parity.sh
```
