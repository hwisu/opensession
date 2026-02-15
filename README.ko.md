# opensession

[![English](https://img.shields.io/badge/lang-English-blue)](README.md)
[![CI](https://github.com/hwisu/opensession/actions/workflows/ci.yml/badge.svg)](https://github.com/hwisu/opensession/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/opensession.svg)](https://crates.io/crates/opensession)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

오픈소스 AI 코딩 세션 매니저. Claude Code, Cursor, Codex, Goose, Aider 등 다양한 AI 도구의 세션을 수집, 탐색, 공유할 수 있습니다.

**웹사이트**: [opensession.io](https://opensession.io)
**GitHub**: [github.com/hwisu/opensession](https://github.com/hwisu/opensession)

## 빠른 시작

### CLI / TUI

```bash
cargo install opensession

opensession            # TUI 실행 (인자 없이)
opensession discover   # 로컬 AI 세션 탐색
opensession upload-all # 발견된 모든 세션 업로드
```

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
| `opensession` | TUI 실행 (서브커맨드 없이) |
| `opensession discover` | 로컬 AI 세션 목록 표시 |
| `opensession upload <file>` | 세션 파일 업로드 |
| `opensession upload-all` | 모든 세션 탐색 후 업로드 |
| `opensession log` | 세션 히스토리 (git-log 스타일) |
| `opensession stats` | AI 사용 통계 |
| `opensession handoff` | 다음 에이전트를 위한 핸드오프 요약 생성 |
| `opensession diff <a> <b>` | 두 세션 비교 |
| `opensession account config` | 설정 조회/변경 |
| `opensession daemon start\|stop\|status` | 백그라운드 데몬 관리 |
| `opensession server status\|verify` | 서버 연결 확인 |
| `opensession hooks install\|uninstall` | Git 훅 관리 |
| `opensession stream enable\|disable` | 실시간 세션 스트리밍 |
| `opensession index` | 로컬 세션 인덱스 빌드 |
| `opensession completion <shell>` | 쉘 자동완성 생성 |

## 설정

### CLI 설정 (`~/.config/opensession/config.toml`)

```bash
opensession account config --server https://opensession.io --api-key osk_xxx --team-id my-team
```

### 데몬 설정 (`~/.config/opensession/daemon.toml`)

TUI 설정 화면이나 파일 직접 편집으로 설정:

```toml
[daemon]
auto_publish = false
publish_on = "manual"        # session_end | realtime | manual
debounce_secs = 5

[server]
url = "https://opensession.io"
api_key = ""

[identity]
nickname = "user"
team_id = ""

[watchers]
claude_code = true
opencode = true
goose = true
aider = true
cursor = false

[privacy]
strip_paths = true
strip_env_vars = true

[git_storage]
method = "native"            # platform_api | native | none
```

### 환경 변수 (서버)

| 변수 | 기본값 | 설명 |
|------|--------|------|
| `OPENSESSION_DATA_DIR` | `data/` | SQLite DB 및 세션 본문 저장 경로 |
| `OPENSESSION_WEB_DIR` | `web/build` | 정적 프론트엔드 파일 경로 |
| `BASE_URL` | `http://localhost:3000` | 외부 공개 URL (설정 시 OAuth 콜백 기준 URL로 사용) |
| `JWT_SECRET` | *(필수)* | JWT 토큰 서명 비밀키 |
| `PORT` | `3000` | HTTP 리슨 포트 |
| `RUST_LOG` | `opensession_server=info,tower_http=info` | 로그 레벨 |

### 로컬 저장소

| 경로 | 설명 |
|------|------|
| `~/.local/share/opensession/local.db` | 로컬 SQLite 캐시 |
| `~/.config/opensession/config.toml` | CLI 설정 |
| `~/.config/opensession/daemon.toml` | 데몬 설정 |

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
