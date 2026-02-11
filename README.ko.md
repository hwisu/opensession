# opensession

[![English](https://img.shields.io/badge/lang-English-blue)](README.md)

AI 코딩 세션 관리를 위한 셀프 호스팅 서버. Claude Code, Cursor, Codex 등 다양한 AI 도구의 세션을 수집, 탐색, 공유할 수 있습니다.

## 빠른 시작

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

세 개의 워크스페이스 크레이트:

| 크레이트 | 바이너리 | 설명 |
|---------|---------|------|
| `server` | `opensession-server` | Axum HTTP 서버, SQLite 저장소 |
| `daemon` | `opensession-daemon` | 백그라운드 파일 감시 및 동기화 에이전트 |
| `worker` | *(별도 빌드)* | Cloudflare Workers 백엔드 (WASM 타겟) |

> CLI, TUI, 파서, 코어 타입은
> [opensession-core](https://github.com/hwisu/opensession-core) 리포지토리에 있습니다.

## 설정

### 환경 변수 (서버)

| 변수 | 기본값 | 설명 |
|------|--------|------|
| `OPENSESSION_DATA_DIR` | `data/` | SQLite DB 및 세션 본문 저장 경로 |
| `OPENSESSION_WEB_DIR` | `web/build` | 정적 프론트엔드 파일 경로 |
| `OPENSESSION_BASE_URL` | `http://localhost:3000` | 외부 공개 URL |
| `PORT` | `3000` | HTTP 리슨 포트 |
| `RUST_LOG` | `opensession_server=info,tower_http=info` | 로그 레벨 (tracing) |

### 로컬 저장소 (서버 없이 사용 시)

서버 없이도 TUI와 데몬은 세션 메타데이터를 로컬에 저장합니다:

| 경로 | 설명 |
|------|------|
| `~/.local/share/opensession/local.db` | 로컬 SQLite 캐시 (세션 메타데이터, git 컨텍스트) |
| `~/.config/opensession/config.toml` | CLI 설정 (서버 URL, API 키, 팀 ID) |
| `~/.config/opensession/daemon.toml` | 데몬 설정 (서버, 아이덴티티, 감시 설정) |

### Docker Compose

포함된 `docker-compose.yml` 구성:

- **포트**: `3000:3000`
- **볼륨**: `opensession-data` → `/data` (SQLite DB + 세션 파일)
- **헬스체크**: `curl -f http://localhost:3000/api/health` (30초 간격)
- **재시작**: `unless-stopped`

Docker 빌드 시 `opensession-core` 리포지토리가 인접 디렉토리에 있어야 합니다:

```
parent/
├── opensession/          # 이 리포지토리
└── opensession-core/     # 빌드 시 필요
```

## API 엔드포인트

### 헬스

| 메서드 | 경로 | 설명 |
|--------|------|------|
| GET | `/api/health` | 헬스 체크 |

### 인증

| 메서드 | 경로 | 설명 |
|--------|------|------|
| POST | `/api/register` | 회원가입 (닉네임 → API 키 발급) |
| POST | `/api/auth/verify` | 토큰 유효성 검증 |
| GET | `/api/auth/me` | 현재 사용자 설정 조회 |
| POST | `/api/auth/regenerate-key` | 새 API 키 생성 |

### 세션

| 메서드 | 경로 | 설명 |
|--------|------|------|
| POST | `/api/sessions` | 세션 업로드 (HAIL JSONL 본문) |
| GET | `/api/sessions` | 세션 목록 (쿼리: `team_id`, `search`, `tool`) |
| GET | `/api/sessions/{id}` | 세션 메타데이터 조회 |
| GET | `/api/sessions/{id}/raw` | 원본 HAIL 파일 다운로드 |

### 팀

| 메서드 | 경로 | 설명 |
|--------|------|------|
| POST | `/api/teams` | 팀 생성 |
| GET | `/api/teams` | 사용자의 팀 목록 |
| GET | `/api/teams/{id}` | 팀 상세 + 세션 조회 |
| PUT | `/api/teams/{id}` | 팀 정보 수정 |
| GET | `/api/teams/{id}/members` | 멤버 목록 |
| POST | `/api/teams/{id}/members` | 멤버 추가 |
| DELETE | `/api/teams/{team_id}/members/{user_id}` | 멤버 제거 |

### 동기화

| 메서드 | 경로 | 설명 |
|--------|------|------|
| GET | `/api/sync/pull` | 세션 풀 (쿼리: `team_id`, `since`, `limit`) |

## 개발

### 사전 요구 사항

- Rust 1.83+
- Node.js 22+ (프론트엔드 빌드용)

### 로컬 실행

```bash
# 서버
cargo run -p opensession-server

# 데몬 (다른 터미널에서)
cargo run -p opensession-daemon
```

### 프론트엔드

```bash
cd web && npm install && npm run dev
```

## 라이선스

MIT
