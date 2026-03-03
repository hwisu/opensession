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
- 레거시 단축 라우트(`/git`, `/gh/*`, `/resolve/*`)는 더 이상 제공되지 않으며 의도적으로 404를 반환
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

## 설치

```bash
cargo install opensession
```

사용자 표면은 `opensession` CLI입니다. 자동 세션 수집(auto-capture)을 쓰려면 daemon 프로세스가 추가로 실행 중이어야 합니다.

## Install-and-Forget 설정

```bash
# 1) CLI 설치
cargo install opensession

# 2) 로컬 설정 진단 (flutter doctor 스타일)
opensession doctor

# 3) 권장 설치값 적용 (변경 전 동의 프롬프트)
opensession doctor --fix

# 선택: fanout 모드를 명시적으로 지정
opensession doctor --fix --fanout-mode hidden_ref

# 자동화/비대화형(non-TTY)
opensession doctor --fix --yes --fanout-mode hidden_ref
```

`doctor`는 내부적으로 기존 setup 파이프라인을 재사용합니다.
`doctor --fix`는 적용 전 setup 계획을 출력하고 동의를 받은 뒤 훅/shim/fanout 변경을 수행합니다.
첫 interactive 적용 시 fanout 저장 모드(`hidden_ref` 또는 `git_notes`)를 선택하며, 선택값은 로컬 git 설정(`.git/config`)의 `opensession.fanout-mode`에 저장됩니다.
비대화형 환경에서는 `--fix`에 `--yes`가 필요하고, 저장된 fanout 모드가 없으면 `--fanout-mode`를 명시해야 합니다.

자동 수집을 위한 daemon 실행:

```bash
# opensession-daemon 바이너리가 있는 경우
opensession-daemon run

# 소스 체크아웃에서 실행하는 경우
cargo run -p opensession-daemon -- run
```

daemon이 없으면 parse/register/share는 수동으로 사용할 수 있지만 백그라운드 자동 수집은 동작하지 않습니다.

## 데스크톱 프리뷰 (Tauri)

기존 Svelte UI를 재사용하는 데스크톱 프리뷰 셸은 [`desktop/`](desktop/README.md)에 있습니다.

```bash
cd desktop
npm install
npm run dev
```

위 명령은 `opensession-server`와 Tauri 데스크톱 창을 함께 실행합니다.

데스크톱 릴리즈는 GitHub Actions `Release` 워크플로에서 수동 실행하며, 이제 crates 릴리즈와 macOS 데스크톱 아티팩트 업로드를 같은 버전 태그로 처리합니다.

## 빠른 시작

```bash
# 첫 사용자용 명령 흐름 출력
opensession docs quickstart

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

## Cleanup 자동화

GitHub/GitLab/Generic 원격에서 hidden ref와 artifact 정리를 자동화할 수 있습니다.

```bash
# cleanup 설정 + 템플릿 초기화
opensession cleanup init --provider auto

# 비대화형 설정
opensession cleanup init --provider auto --yes

# cleanup 상태 + janitor 미리보기
opensession cleanup status

# 기본은 dry-run
opensession cleanup run

# 실제 삭제 적용
opensession cleanup run --apply
```

기본값:

- hidden ref TTL: 30일
- artifact branch TTL: 30일
- GitHub/GitLab 설정 시 PR/MR 갱신마다 artifact branch를 갱신하고 리뷰 코멘트를 남기는 session-review 자동화 템플릿도 함께 생성됩니다.

민감한 저장소는 즉시 정리 모드를 권장합니다.

```bash
opensession cleanup init --provider auto --hidden-ttl-days 0 --artifact-ttl-days 0 --yes
```

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

## 실패 복구 가이드

자주 발생하는 실패 시그니처와 즉시 복구 명령:

1. local URI로 `share --web` 실행:
```bash
opensession share os://src/local/<sha256> --git --remote origin
opensession share os://src/git/<remote_b64>/ref/<ref_enc>/path/<path...> --web
```
2. `share --git`에서 remote 누락:
```bash
opensession share os://src/local/<sha256> --git --remote origin
```
3. git 저장소 밖에서 `share --git` 실행:
```bash
cd <your-repo>
opensession share os://src/local/<sha256> --git --remote origin
```
4. `.opensession/config.toml` 없이 `share --web` 실행:
```bash
opensession config init --base-url https://opensession.io
opensession config show
```
5. 비정규 입력으로 `register` 실행:
```bash
opensession parse --profile codex ./raw-session.jsonl --out ./session.hail.jsonl
opensession register ./session.hail.jsonl
```
6. parser/input 불일치로 `parse` 실패:
```bash
opensession parse --help
opensession parse --profile codex ./raw-session.jsonl --preview
```
7. `view` 타겟 해석 실패:
```bash
opensession view os://src/... --no-open
opensession view ./session.hail.jsonl --no-open
opensession view HEAD
```
8. cleanup 설정 전에 `cleanup run` 실행:
```bash
opensession cleanup init --provider auto
opensession cleanup run
```

처음 사용자 5분 복귀 경로:
```bash
opensession doctor
opensession doctor --fix
opensession parse --profile codex ./raw-session.jsonl --out ./session.hail.jsonl
opensession register ./session.hail.jsonl
opensession share os://src/local/<sha256> --git --remote origin
```

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
