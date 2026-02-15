# opensession

[![English](https://img.shields.io/badge/lang-English-blue)](README.md)
[![CI](https://github.com/hwisu/opensession/actions/workflows/ci.yml/badge.svg)](https://github.com/hwisu/opensession/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/opensession.svg)](https://crates.io/crates/opensession)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

오픈소스 AI 코딩 세션 매니저. Claude Code, Cursor, Codex, Goose, Aider 등 다양한 AI 도구의 세션을 수집, 탐색, 공유할 수 있습니다.

**웹사이트**: [opensession.io](https://opensession.io)
**GitHub**: [github.com/hwisu/opensession](https://github.com/hwisu/opensession)

## 빠른 시작

### CLI

```bash
cargo install opensession

opensession --help
opensession session handoff --last
opensession publish upload-all
opensession daemon start --agent claude-code --repo .
```

수동 로컬 탐색 모드(TUI):

```bash
opensession      # 전체 로컬 세션
opensession .    # 현재 git 레포 범위
```

### 배포 프로필

| 항목 | Docker (Axum 서버) | Worker (Wrangler) |
|------|---------------------|-------------------|
| 주 목적 | 팀 협업 | 개인 공유 |
| 비로그인 시 홈(`/`) | 랜딩 페이지 | 랜딩 페이지 |
| 로그인 시 홈(`/`) | 세션 목록 | 세션 목록 |
| 팀 API (`/api/teams*`, `/api/invitations*`, `/api/sync/pull`) | 활성화 | `ENABLE_TEAM_API=false`일 때 비활성화 |
| 팀 UI (`/teams`, `/invitations`) | 활성화 | 숨김/비활성화 |
| 업로드 모드 | 팀 대상 업로드 | 개인 업로드 (`team_id=personal`) |

- 웹 빌드 프로필: `VITE_APP_PROFILE=docker|worker`
- 저장소 기본값:
  - `docker-compose.yml`: `OPENSESSION_PUBLIC_FEED_ENABLED=false` (익명 `GET /api/sessions` 차단)
  - `wrangler.toml`: `ENABLE_TEAM_API=false`

### 셀프 호스팅 서버

```bash
docker compose up -d
# → http://localhost:3000
# 최초 등록한 사용자가 관리자(admin)가 됩니다.
```

## 아키텍처

```
┌─────────┐    ┌────────┐    ┌──────────────────┐
│  CLI /  │───▶│ daemon │───▶│ server (Axum)    │
│  TUI    │    │ (watch │    │ SQLite + disk     │
└─────────┘    │ +sync) │    │ :3000             │
               └────────┘    └──────────────────┘
```

단일 Cargo 워크스페이스, 12개 크레이트:

| 크레이트 | 설명 |
|---------|------|
| `core` | HAIL 도메인 모델 (순수 타입, 검증) |
| `parsers` | 7개 AI 도구용 세션 파서 |
| `api` | 공유 API 타입, SQL 빌더, 서비스 로직 |
| `api-client` | 서버 통신용 HTTP 클라이언트 |
| `local-db` | 로컬 SQLite 데이터베이스 레이어 |
| `git-native` | `gix` 기반 Git 연산 |
| `server` | Axum HTTP 서버, SQLite 저장소 |
| `daemon` | 백그라운드 파일 감시 및 동기화 에이전트 |
| `cli` | CLI 진입점 (바이너리: `opensession`) |
| `tui` | 세션 탐색용 터미널 UI |
| `worker` | Cloudflare Workers 백엔드 (WASM, 워크스페이스에서 제외) |
| `e2e` | E2E 테스트 |

## CLI 명령어

| 명령어 | 설명 |
|--------|------|
| `opensession` / `opensession .` | 로컬 인터랙티브 모드 실행 (전체 / 현재 레포 범위) |
| `opensession session handoff` | 다음 에이전트를 위한 핸드오프 요약 생성 |
| `opensession publish upload <file>` | 세션 파일 업로드 |
| `opensession publish upload-all` | 모든 세션 탐색 후 업로드 |
| `opensession daemon start\|stop\|status\|health` | 데몬 실행/중지/상태 관리 |
| `opensession daemon select --agent ... --repo ...` | 감시 에이전트/레포 선택 |
| `opensession daemon show` | 현재 데몬 대상 확인 |
| `opensession daemon stream-push --agent <agent>` | 내부 훅 대상 명령 |
| `opensession account connect --server --api-key [--team-id]` | 서버/계정 빠른 연결 |
| `opensession account team --id <team-id>` | 기본 팀 설정 |
| `opensession account status\|verify` | 서버 연결/인증 확인 |
| `opensession docs completion <shell>` | 쉘 자동완성 생성 |

숨김/레거시 알리아스는 제거되었으며, 위 표의 명령 집합이 현재 표준 CLI 표면입니다.

## 설정

### 통합 설정 (`~/.config/opensession/opensession.toml`)

```bash
opensession account connect --server https://opensession.io --api-key osk_xxx --team-id my-team
```

전역 설정은 `opensession.toml`만 사용합니다. 레거시 폴백(`daemon.toml`, `config.toml`)은 더 이상 읽지 않습니다.

### 데몬 설정 (`~/.config/opensession/opensession.toml`)

TUI 설정 화면이나 파일 직접 편집으로 설정:

```toml
[daemon]
auto_publish = false         # TUI "Daemon Capture" 토글이 관리
publish_on = "manual"        # ON => session_end, OFF => manual
debounce_secs = 5

[server]
url = "https://opensession.io"
api_key = ""

[identity]
nickname = "user"
team_id = ""

[watchers]
custom_paths = [
  "~/.claude/projects",
  "~/.codex/sessions",
  "~/.local/share/opencode/storage/session",
  "~/.cline/data/tasks",
  "~/.local/share/amp/threads",
  "~/.gemini/tmp",
  "~/Library/Application Support/Cursor/User",
  "~/.config/Cursor/User",
]

[privacy]
strip_paths = true
strip_env_vars = true

[git_storage]
method = "native"            # platform_api | native | none
```

에이전트별 watcher 토글은 하위 호환을 위해 읽기만 지원하며,
새 설정 저장 시에는 `watchers.custom_paths`만 기록됩니다.

### 환경 변수 (서버)

| 변수 | 기본값 | 설명 |
|------|--------|------|
| `OPENSESSION_DATA_DIR` | `data/` | SQLite DB 및 세션 본문 저장 경로 |
| `OPENSESSION_WEB_DIR` | `web/build` | 정적 프론트엔드 파일 경로 |
| `BASE_URL` | `http://localhost:3000` | 외부 공개 URL (설정 시 OAuth 콜백 기준 URL로 사용) |
| `OPENSESSION_PUBLIC_FEED_ENABLED` | `true` | `false`일 때 `GET /api/sessions` 익명 접근 차단 |
| `JWT_SECRET` | *(필수)* | JWT 토큰 서명 비밀키 |
| `PORT` | `3000` | HTTP 리슨 포트 |
| `RUST_LOG` | `opensession_server=info,tower_http=info` | 로그 레벨 |

### 로컬 저장소

| 경로 | 설명 |
|------|------|
| `~/.local/share/opensession/local.db` | 로컬 SQLite 캐시 |
| `~/.config/opensession/opensession.toml` | CLI/데몬 통합 설정 |

## API 엔드포인트

### 헬스

| 메서드 | 경로 | 설명 |
|--------|------|------|
| GET | `/api/health` | 헬스 체크 |

### 인증

| 메서드 | 경로 | 설명 |
|--------|------|------|
| POST | `/api/register` | 회원가입 (닉네임 → API 키 발급) |
| POST | `/api/auth/register` | 이메일/비밀번호 회원가입 |
| POST | `/api/auth/login` | 이메일/비밀번호 로그인 |
| POST | `/api/auth/refresh` | 액세스 토큰 갱신 |
| POST | `/api/auth/logout` | 로그아웃 (리프레시 토큰 폐기) |
| POST | `/api/auth/verify` | 토큰 유효성 검증 |
| GET | `/api/auth/me` | 현재 사용자 설정 조회 |
| POST | `/api/auth/regenerate-key` | 새 API 키 생성 |
| PUT | `/api/auth/password` | 비밀번호 변경 |

### OAuth

| 메서드 | 경로 | 설명 |
|--------|------|------|
| GET | `/api/auth/providers` | 사용 가능한 인증 제공자 목록 |
| GET | `/api/auth/oauth/{provider}` | OAuth 제공자로 리다이렉트 |
| GET | `/api/auth/oauth/{provider}/callback` | OAuth 콜백 |
| POST | `/api/auth/oauth/{provider}/link` | 기존 계정에 OAuth 연결 |

### 세션

| 메서드 | 경로 | 설명 |
|--------|------|------|
| POST | `/api/sessions` | 세션 업로드 (HAIL JSONL 본문) |
| GET | `/api/sessions` | 세션 목록 (쿼리: `team_id`, `search`, `tool`) |
| GET | `/api/sessions/{id}` | 세션 상세 조회 |
| DELETE | `/api/sessions/{id}` | 세션 삭제 (소유자만) |
| GET | `/api/sessions/{id}/raw` | 원본 HAIL 파일 다운로드 |

### 팀

| 메서드 | 경로 | 설명 |
|--------|------|------|
| POST | `/api/teams` | 팀 생성 |
| GET | `/api/teams` | 사용자의 팀 목록 |
| GET | `/api/teams/{id}` | 팀 상세 + 세션 조회 |
| PUT | `/api/teams/{id}` | 팀 정보 수정 |
| GET | `/api/teams/{id}/stats` | 팀 사용 통계 |
| GET | `/api/teams/{id}/members` | 멤버 목록 |
| POST | `/api/teams/{id}/members` | 멤버 추가 |
| DELETE | `/api/teams/{id}/members/{user_id}` | 멤버 제거 |
| POST | `/api/teams/{id}/invite` | 멤버 초대 (이메일/OAuth) |

### 초대

| 메서드 | 경로 | 설명 |
|--------|------|------|
| GET | `/api/invitations` | 대기 중인 초대 목록 |
| POST | `/api/invitations/{id}/accept` | 초대 수락 |
| POST | `/api/invitations/{id}/decline` | 초대 거절 |

### 동기화

| 메서드 | 경로 | 설명 |
|--------|------|------|
| GET | `/api/sync/pull` | 세션 풀 (쿼리: `team_id`, `cursor`, `limit`) |

### 문서

| 메서드 | 경로 | 설명 |
|--------|------|------|
| GET | `/docs` | API 문서 |
| GET | `/llms.txt` | LLM 친화적 문서 |

## Docker

```bash
# 사전 빌드된 이미지 사용
docker run -p 3000:3000 -v opensession-data:/data \
  -e JWT_SECRET=your-secret-here \
  ghcr.io/hwisu/opensession

# 또는 docker compose
docker compose up -d
```

모노레포 자체 빌드 — 외부 의존성 불필요.

## 개발

### 사전 요구 사항

- Rust 1.85+
- Node.js 22+ (프론트엔드)

### 로컬 실행

```bash
# 서버
cargo run -p opensession-server

# 데몬 (별도 터미널)
cargo run -p opensession-daemon

# TUI
cargo run -p opensession-tui

# 프론트엔드 개발 서버
cd web && npm install && npm run dev
```

### 테스트

```bash
cargo test --workspace                        # 전체 워크스페이스 테스트
cargo test -p opensession-core                # 단일 크레이트
cd crates/worker && cargo check --target wasm32-unknown-unknown  # Worker
```

## HAIL 포맷

**HAIL** (Human-AI Interaction Log)은 AI 코딩 세션을 기록하는 오픈 JSONL 포맷입니다.

```jsonl
{"v":"hail/0.1","tool":"claude-code","model":"opus-4","ts":"2025-01-01T00:00:00Z"}
{"role":"human","content":"인증 버그 수정해줘"}
{"role":"agent","content":"수정하겠습니다...","tool_calls":[...]}
{"type":"file_edit","path":"src/auth.rs","diff":"..."}
```

## 기여

[CONTRIBUTING.md](CONTRIBUTING.md)를 참고하세요.

## 라이선스

MIT
