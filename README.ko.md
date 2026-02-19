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
opensession publish upload-all
opensession daemon start --repo .
```

수동 로컬 탐색 모드(TUI):

```bash
opensession      # 전체 로컬 세션
opensession .    # 현재 git 레포 범위
```

## 런타임 프로필

| 항목 | Server (Axum) | Worker (Wrangler) |
|------|----------------|-------------------|
| 홈(`/`) | 게스트 랜딩, 로그인 후 세션 목록 | 공개 세션 목록 |
| 업로드 UI(`/upload`) | 사용 가능 | 비활성(read-only) |
| API 표면 | `/api/health`, `/api/sessions*` | `/api/health`, `/api/sessions*` |
| 인증 라우트 | 활성 | 비활성 |
| 팀/초대/싱크 라우트 | 비활성 | 비활성 |

웹 빌드 프로필:
- `VITE_APP_PROFILE=server|worker`

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
| `local-db` | 로컬 SQLite 레이어 |
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
| `opensession session handoff` | 다음 에이전트용 핸드오프 요약 생성 |
| `opensession publish upload <file>` | 세션 파일 업로드 |
| `opensession publish upload-all` | 모든 세션 탐색 후 업로드 |
| `opensession publish upload <file> --git` | git-native 브랜치(`opensession/sessions`)에 저장 |
| `opensession daemon start\|stop\|status\|health` | 데몬 실행/중지/상태 |
| `opensession daemon select --repo ...` | 감시 경로/레포 선택 |
| `opensession daemon show` | 현재 감시 대상 확인 |
| `opensession account connect` | 서버 URL/API 키 설정(선택) |
| `opensession account status\|verify` | 서버 연결 상태 확인 |
| `opensession docs completion <shell>` | 쉘 자동완성 생성 |

## 설정

표준 설정 파일:
- `~/.config/opensession/opensession.toml`

로컬 캐시 DB:
- `~/.local/share/opensession/local.db`

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
| POST | `/api/sessions` | HAIL 세션 업로드 (인증 필요) |
| GET | `/api/sessions` | 세션 목록 조회 |
| GET | `/api/sessions/{id}` | 세션 상세 조회 |
| GET | `/api/sessions/{id}/raw` | 원본 HAIL JSONL 다운로드 |
| DELETE | `/api/sessions/{id}` | 세션 삭제 |

## 셀프 호스팅 서버

```bash
cargo run -p opensession-server
# -> http://localhost:3000
```

주요 환경 변수:

| 변수 | 기본값 | 설명 |
|------|--------|------|
| `OPENSESSION_DATA_DIR` | `data/` | SQLite DB 및 세션 저장 경로 |
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
