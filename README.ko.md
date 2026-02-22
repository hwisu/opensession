# opensession

[![English](https://img.shields.io/badge/lang-English-blue)](README.md)
[![CI](https://github.com/hwisu/opensession/actions/workflows/ci.yml/badge.svg)](https://github.com/hwisu/opensession/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/opensession.svg)](https://crates.io/crates/opensession)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

OpenSession은 AI 세션 로그를 로컬 우선(local-first)으로 기록/등록/공유/검토하는 워크플로입니다.

웹: [opensession.io](https://opensession.io)  
문서: [opensession.io/docs](https://opensession.io/docs)

## DX 리셋 v1

CLI/Web/API 계약은 3가지 동작으로 정리되었습니다.

- `register`: canonical HAIL JSONL을 로컬 저장소에 등록 (네트워크 부작용 없음)
- `share`: Source URI를 공유 가능한 출력으로 변환
- `handoff`: 불변(immutable) 아티팩트를 생성하고 alias를 관리

레거시 표면은 제거되었습니다.

- `opensession publish ...` 제거
- `opensession session handoff ...` 제거
- `/git`, `/gh/*` 라우트 제거
- `/api/ingest/preview` 제거 (`/api/parse/preview` 사용)

## URI 모델

- `os://src/local/<sha256>`
- `os://src/gh/<owner>/<repo>/ref/<ref_enc>/path/<path...>`
- `os://src/gl/<project_b64>/ref/<ref_enc>/path/<path...>`
- `os://src/git/<remote_b64>/ref/<ref_enc>/path/<path...>`
- `os://artifact/<sha256>`

인코딩 규칙:

- `ref_enc`: RFC3986 percent-encoding
- `project_b64`, `remote_b64`: base64url(no padding)

## 빠른 시작

```bash
# agent-native 로그 -> canonical HAIL JSONL
opensession parse --profile codex ./raw-session.jsonl > ./session.hail.jsonl

# 로컬 object store 등록
opensession register ./session.hail.jsonl
# -> os://src/local/<sha256>

# 원본 바이트 확인
opensession cat os://src/local/<sha256>

# 요약 메타데이터 확인
opensession inspect os://src/local/<sha256>
```

## 공유(share)

```bash
# local URI -> git 공유 가능 Source URI
opensession share os://src/local/<sha256> --git --remote origin

# 선택적 네트워크 변경
opensession share os://src/local/<sha256> --git --remote origin --push

# remote-resolvable URI -> 웹 URL
opensession config init --base-url https://opensession.io
opensession share os://src/git/<remote_b64>/ref/<ref_enc>/path/<path...> --web
```

`share --web`는 `.opensession/config.toml`이 반드시 필요합니다.

## handoff

```bash
# 불변 artifact 생성
opensession handoff build --from os://src/local/<sha256> --pin latest
# -> os://artifact/<sha256>

# 출력 표현 선택
opensession handoff artifacts get os://artifact/<sha256> --format canonical --encode jsonl

# 해시/내용 검증
opensession handoff artifacts verify os://artifact/<sha256>

# alias 관리
opensession handoff artifacts pin latest os://artifact/<sha256>
opensession handoff artifacts unpin latest

# 삭제 정책: unpinned만 삭제 가능
opensession handoff artifacts rm os://artifact/<sha256>
```

## Canonical 웹 라우트

- `/src/gh/<owner>/<repo>/ref/<ref_enc>/path/<path...>`
- `/src/gl/<project_b64>/ref/<ref_enc>/path/<path...>`
- `/src/git/<remote_b64>/ref/<ref_enc>/path/<path...>`

## API 표면(v1)

- `GET /api/health`
- `GET /api/capabilities`
- `POST /api/parse/preview`
- `GET /api/sessions`
- `GET /api/sessions/{id}`
- `GET /api/sessions/{id}/raw`
- `DELETE /api/admin/sessions/{id}` (`X-OpenSession-Admin-Key` 필요)

## 로컬 개발 검증

```bash
# 필수 훅 게이트
./.githooks/pre-commit
./.githooks/pre-push
```

```bash
# 웹 런타임 검증
npx wrangler dev --ip 127.0.0.1 --port 8788 --persist-to .wrangler/state
BASE_URL=http://127.0.0.1:8788 npx playwright test e2e/git-share.spec.ts --config playwright.config.ts
```
