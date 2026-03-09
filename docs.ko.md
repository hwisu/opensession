# 문서

OpenSession은 AI 세션 트레이스를 등록하고, 공유하고, 검토하는 로컬 우선(local-first) 워크플로입니다.
공개 계약은 CLI, Web, API가 함께 쓰는 단일 Source URI 모델입니다.

## 문서 맵

- 루트 빠른 참조: `README.md` / `README.ko.md`
- 이 문서(`docs.ko.md`): 제품 계약과 명령 의미론
- 개발/CI 정합성 런북: `docs/development-validation-flow.md`
- 하네스 실패 루프 정책: `docs/harness-auto-improve-loop.md`
- 파서 소스/재사용 경계: `docs/parser-source-matrix.md`

## 시작하기

핵심 원칙:

- 하나의 개념에는 하나의 이름을 쓴다.
- 하나의 식별자에는 하나의 URI를 쓴다.
- 암묵적인 네트워크 변경은 하지 않는다.
- 기본값을 사용하더라도 출력에 드러나야 한다.

초보자용 3단계 빠른 시작:

```bash
# 첫 사용자용 명령 흐름 출력
opensession docs quickstart

# 1) CLI 설치
cargo install opensession

# 2) 로컬 설정 진단 (flutter doctor 스타일)
opensession doctor

# 3) 명시적 확인 후 설정 적용
opensession doctor --fix --profile local
```

- `doctor --fix`는 훅/shim/fanout 변경을 적용하기 전에 계획을 출력하고 확인을 받습니다.
- 자동화나 비대화형 셸에서는 명시적 모드와 승인 플래그를 함께 사용하세요:
  `opensession doctor --fix --yes --profile local --fanout-mode hidden_ref`

빠른 경로:

```bash
# 1) agent-native 로그를 canonical HAIL JSONL로 변환
opensession parse --profile codex ./raw-session.jsonl > ./session.hail.jsonl

# 2) canonical 세션을 로컬 object store에 등록
opensession register ./session.hail.jsonl
# -> os://src/local/<sha256>

# 3) 로컬 canonical 바이트 다시 읽기
opensession cat os://src/local/<sha256>

# 4) summary 메타데이터 확인
opensession inspect os://src/local/<sha256>
```

설치:

```bash
cargo install opensession
```

설치 프로필:

- `local`(기본): backup/summary/handoff 중심 CLI 로컬 우선 경로
- `app`: 데스크톱 앱 사용자용 프로필 (`opensession doctor --fix --profile app --open-target app`)

자동 수집 참고:

- `opensession`은 parse/register/share/handoff를 담당합니다.
- 백그라운드 자동 수집은 daemon 프로세스(`opensession-daemon run`)가 실행 중이어야 합니다.

레포 개발 툴체인:

- 로컬 검증 훅은 `mise`를 통해 실행됩니다.
- `./.githooks/pre-commit`, `./.githooks/pre-push` 전에 레포 루트에서 `mise install`을 실행하세요.
- 데스크톱 사전 점검 게이트: `node scripts/validate/desktop-build-preflight.mjs --mode local`

로컬 object storage:

- 레포 내부: `.opensession/objects/sha256/ab/cd/<hash>.jsonl`
- 레포 외부: `~/.local/share/opensession/objects/sha256/ab/cd/<hash>.jsonl`

해시 정책:

- canonical HAIL JSONL 바이트의 SHA-256

## 데스크톱 런타임 Summary 계약 (v3)

데스크톱 IPC/runtime settings는 typed summary 계약을 사용합니다.

- `summary.provider.id|endpoint|model`
- `summary.prompt.template`
- `summary.response.style|shape`
- `summary.storage.trigger|backend`
- `summary.source_mode`
- `vector_search.enabled|provider|model|endpoint|granularity|chunk_size_lines|chunk_overlap_lines|top_k_chunks|top_k_sessions`

데스크톱 로컬 제약:

- `auth_enabled=false` 런타임은 의도적으로 account/auth UI를 숨깁니다.
- 데스크톱 로컬 런타임에서는 `summary.source_mode`가 `session_only`로 고정됩니다.
- `session_or_git_changes`는 CI/CLI 같은 비-데스크톱 런타임 컨텍스트용입니다.
- 기본 summary storage backend는 `hidden_ref`입니다.
- `hidden_ref`를 써도 list/search 메타데이터와 vector index 메타데이터는 로컬 SQLite(`OPENSESSION_LOCAL_DB_PATH` 또는 기본 `~/.local/share/opensession/local.db`)에 인덱싱됩니다.
- Settings의 runtime response preview는 모델 출력이 아니라 결정론적 로컬 샘플 렌더링입니다.

데스크톱 로컬 확장:

- HTTP docs 라우트가 없어도 `/docs`는 desktop IPC(`desktop_get_docs_markdown`)에서 해석할 수 있습니다.
- 벡터 검색은 이벤트/라인 청크 인덱싱과 로컬 Ollama 임베딩(기본 `bge-m3`)을 사용합니다.
- 벡터 검색 활성화는 명시적입니다. 먼저 모델 설치가 끝나야 합니다(`desktop_vector_preflight`, `desktop_vector_install_model`).
- 인덱싱도 명시적이며 상태를 관찰할 수 있습니다(`desktop_vector_index_rebuild`, `desktop_vector_index_status`).

## Git을 통한 공유

`register`는 로컬 전용입니다. 원격 공유는 `share`로 명시적으로 수행합니다.

```bash
# 로컬 source -> 원클릭 git share URI 흐름
opensession share os://src/local/<sha256> --quick

# 선택적 네트워크 변경
opensession share os://src/local/<sha256> --git --remote origin --push
```

`share --git` / `share --quick` 규칙:

- `--quick`은 remote를 자동 감지합니다(`origin` 우선, 단일 remote fallback)
- `--git`은 명시적 `--remote <name|url>`가 필요합니다
- 기본 ref: `refs/opensession/branches/<branch_b64url>`
- 기본 path: `sessions/<sha256>.jsonl`
- `--push`를 생략하면 네트워크 변경 없이 실행 가능한 push 명령만 출력합니다
- `--quick`은 첫 푸시 때 한 번 확인을 받고, 레포별 동의를 `.git/config`의 `opensession.share.auto-push-consent=true`에 저장합니다
- 새 write에는 더 이상 레거시 고정 ref `refs/heads/opensession/sessions`를 사용하지 않습니다

설치 후 그대로 쓰는 설정:

```bash
opensession doctor
opensession doctor --fix --profile local
# interactive 셸에서 선택 모드를 명시하고 싶다면
opensession doctor --fix --profile local --fanout-mode hidden_ref
# automation/non-interactive
opensession doctor --fix --yes --profile local --fanout-mode hidden_ref --open-target web
```

- `doctor` check 모드는 내부 setup check로, `doctor --fix`는 내부 setup apply 흐름으로 연결됩니다.
- `doctor --fix`는 명시적 승인이 필요합니다. interactive 기본 프롬프트 또는 자동화용 `--yes`를 사용합니다.
- 현재 레포에 OpenSession 관리 `pre-push` 훅을 설치/업데이트합니다.
- `~/.local/share/opensession/bin/opensession`에 OpenSession shim을 설치/업데이트합니다.
- fanout 모드가 아직 설정되지 않은 interactive 셸에서는 첫 적용 시 `hidden_ref` 또는 `git_notes`를 선택하도록 묻고, 결과를 로컬 git config(`opensession.fanout-mode`)에 저장합니다.
- 비대화형 적용은 레포에 저장된 `opensession.fanout-mode`가 없으면 명시적 `--fanout-mode`가 필요합니다.
- open target 기본값은 profile을 따릅니다(`local -> web`, `app -> app`).
- `doctor` 출력에는 `~/.config/opensession/daemon.pid` 기준 daemon 상태가 포함됩니다.
- daemon 시작: `opensession-daemon run` (소스 체크아웃에서는 `cargo run -p opensession-daemon -- run`)
- `remote.<name>.push`는 수정하지 않습니다.
- hook fanout push는 best-effort이며 경고만 출력합니다.
- fanout helper가 없거나 fanout push가 실패하면 push를 실패시키려면 `OPENSESSION_STRICT=1`을 사용하세요.
- PR 자동화는 현재 same-repo 비봇 PR만 지원합니다.
- merge/branch delete cleanup은 ledger ref를 즉시 제거하고, 실제 object 제거는 remote GC 정책을 따릅니다.

`share --web` 규칙:

```bash
opensession config init --base-url https://opensession.io
opensession config show
opensession share os://src/git/<remote_b64>/ref/<ref_enc>/path/<path...> --web
```

- `share --web`는 명시적 `.opensession/config.toml`이 필요합니다
- 로컬 URI에 `--web`을 붙이면 후속 조치(`share --git`)와 함께 거부됩니다
- 사람이 읽는 출력에서는 canonical URL이 첫 줄에 인쇄됩니다

## Cleanup 자동화

OpenSession은 서버 인프라를 바꾸지 않고도 사용자 저장소에서 hidden ref cleanup을 구성할 수 있습니다.

```bash
# provider-aware cleanup 템플릿/설정 초기화
opensession cleanup init --provider auto

# 비대화형 설정
opensession cleanup init --provider auto --yes

# 설정 + janitor dry-run 요약 확인
opensession cleanup status

# dry-run (기본)
opensession cleanup run

# 실제 삭제 적용
opensession cleanup run --apply
```

기본값:

- hidden ref TTL: 30일
- artifact branch TTL: 30일
- 기본값은 ephemeral PR/MR artifact branch이며 리뷰가 닫히면 삭제됩니다. `--session-archive-branch <branch>`를 설정하면 `pr/sessions` 같은 전용 archive branch에 immutable snapshot을 계속 보관합니다.

민감한 저장소용:

```bash
opensession cleanup init --provider auto --hidden-ttl-days 0 --artifact-ttl-days 0 --yes

# 전용 브랜치에 리뷰 스냅샷 영구 보관
opensession cleanup init --provider auto --session-archive-branch pr/sessions --yes
```

프로바이더 매트릭스:

- GitHub: `.github/workflows/opensession-cleanup.yml`와 `.github/workflows/opensession-session-review.yml`을 생성합니다. 기본값은 ephemeral `opensession/pr-<number>-sessions` 브랜치를 PR 동안만 유지하고 PR close 시 삭제합니다. `--session-archive-branch <branch>`를 설정하면 `pr/sessions` 같은 전용 archive branch에 immutable snapshot을 저장합니다.
- GitLab: `.gitlab/opensession-cleanup.yml`와 `.gitlab/opensession-session-review.yml`을 생성합니다. `.gitlab-ci.yml`은 OpenSession 관리 마커 블록이 있을 때만(또는 새 파일일 때만) 갱신합니다. MR 파이프라인은 `opensession/mr-<iid>-sessions`를 게시/갱신하거나, `--session-archive-branch`가 설정된 경우 해당 archive branch를 사용합니다.
- Generic git: cron/system scheduler 연동용 `.opensession/cleanup/cron.example`를 생성합니다.
- session-review 코멘트에는 `Reviewer Quick Digest`가 포함되며, Q&A 발췌(`Question | Answer` 행), 수정 파일 요약, 추가/수정 테스트가 함께 표시됩니다.

## 개발 및 검증

정식 검증 흐름(훅, API/worker/web/desktop E2E, CI 정합성, artifact 정책):

- `docs/development-validation-flow.md`

빠른 로컬 게이트 명령:

```bash
./.githooks/pre-commit
./.githooks/pre-push
```

GitHub CI 분리 정책:

- `.github/workflows/ci.yml`은 빠른 PR/main 게이트만 유지합니다.
- `.github/workflows/ci-deep.yml`이 장시간 audit/E2E/desktop 검증을 정기/수동 트리거로 담당합니다.

데스크톱 빌드 정책:

- deep CI에서 Linux 데스크톱 번들 빌드 검증(`desktop-bundle-verify`)을 필수로 수행합니다.
- macOS 데스크톱 릴리즈 타깃은 `universal-apple-darwin`만 허용합니다.
- universal 아키텍처는 `lipo -archs`로 검증하며 반드시 `x86_64`와 `arm64`를 모두 포함해야 합니다.
- 정기/수동 `Desktop Dry Run` 워크플로는 no-sign 데스크톱 번들을 검증하고 diagnostics/metrics artifact를 업로드합니다.

릴리즈 서명 체크리스트(수동 secret 준비):

- `APPLE_CERTIFICATE`
- `APPLE_CERTIFICATE_PASSWORD`
- `APPLE_SIGNING_IDENTITY`
- `APPLE_ID`
- `APPLE_PASSWORD`
- `APPLE_TEAM_ID`

## 실패 복구

일반적인 온보딩 흐름이 실패했을 때 아래 명령을 사용하세요.

1. `share --web`에 로컬 URI를 넣은 경우:
```bash
opensession share os://src/local/<sha256> --git --remote origin
opensession share os://src/git/<remote_b64>/ref/<ref_enc>/path/<path...> --web
```
2. `share --git`에 `--remote`가 없는 경우:
```bash
opensession share os://src/local/<sha256> --quick
```
3. `share --git`을 git 저장소 밖에서 실행한 경우:
```bash
cd <repo>
opensession share os://src/local/<sha256> --quick
```
4. `share --web`에 config가 없는 경우:
```bash
opensession config init --base-url https://opensession.io
opensession config show
```
5. `register`가 non-canonical 입력을 거부한 경우:
```bash
opensession parse --profile codex ./raw-session.jsonl --out ./session.hail.jsonl
opensession register ./session.hail.jsonl
```
6. `parse`에서 parser/input이 맞지 않는 경우:
```bash
opensession parse --help
opensession parse --profile codex ./raw-session.jsonl --preview
```
7. `view` target 해석이 실패한 경우:
```bash
opensession view os://src/... --no-open
opensession view ./session.hail.jsonl --no-open
opensession view HEAD
```
8. `cleanup run` 전에 초기화를 하지 않은 경우:
```bash
opensession cleanup init --provider auto
opensession cleanup run
```

5분 복구 경로:

```bash
opensession doctor
opensession doctor --fix --profile local
opensession parse --profile codex ./raw-session.jsonl --out ./session.hail.jsonl
opensession register ./session.hail.jsonl
opensession share os://src/local/<sha256> --quick
```

## 타임라인 검토

canonical 웹 라우트:

- `/src/gh/<owner>/<repo>/ref/<ref_enc>/path/<path...>`
- `/src/gl/<project_b64>/ref/<ref_enc>/path/<path...>`
- `/src/git/<remote_b64>/ref/<ref_enc>/path/<path...>`

레거시 단축 라우트는 예약되어 있으며 404를 반환합니다.

- `/git`
- `/gh/*`
- `/resolve/*`

서버 parse preview 엔드포인트:

- `POST /api/parse/preview`

## Review 뷰

`opensession view`는 리뷰 중심 웹 진입점입니다.

```bash
# Source URI -> /src/*
opensession view os://src/gl/<project_b64>/ref/<ref_enc>/path/<path...>

# 로컬 source URI / jsonl 파일 -> /review/local/<id>
opensession view os://src/local/<sha256>
opensession view ./session.hail.jsonl

# commit/ref/range -> commit-linked local review bundle
opensession view HEAD
opensession view main..feature/my-branch
```

기본 모드는 web입니다. URL만 출력하려면 `--no-open`을 사용하세요.

로컬 `view` 대상은 등록된 git credential이 필요하지 않습니다.
로컬 git object / 로컬 source byte를 사용해 local review bundle을 만들기 때문입니다.
commit-linked local review page는 Q&A 내용 발췌, 수정 파일, 추가/수정 테스트를 포함하는 `Reviewer Quick Digest` 패널을 노출합니다.

## Handoff

handoff artifact는 immutable입니다. `build`는 매번 새 artifact URI를 생성합니다.

```bash
# immutable artifact 생성
opensession handoff build --from os://src/local/<sha256> --pin latest
# -> os://artifact/<sha256>

# payload 표현 읽기
opensession handoff artifacts get os://artifact/<sha256> --format canonical --encode jsonl

# 결정론적 해시 검증
opensession handoff artifacts verify os://artifact/<sha256>

# alias 제어
opensession handoff artifacts pin latest os://artifact/<sha256>
opensession handoff artifacts unpin latest

# 제거 정책 (unpinned만 허용)
opensession handoff artifacts rm os://artifact/<sha256>
```

v1에는 refresh/update 명령이 없습니다. 다시 build하고 pin alias를 옮기면 됩니다.

## 선택적 UI

CLI가 정식 운영 표면입니다.
Web과 TUI는 같은 URI 계약 위에서 동작하는 선택적 인터페이스입니다.

## 개념

source / artifact 식별자:

- `os://src/local/<sha256>`
- `os://src/gh/<owner>/<repo>/ref/<ref_enc>/path/<path...>`
- `os://src/gl/<project_b64>/ref/<ref_enc>/path/<path...>`
- `os://src/git/<remote_b64>/ref/<ref_enc>/path/<path...>`
- `os://artifact/<sha256>`

인코딩 규칙:

- `ref_enc`: RFC3986 percent-encoding
- `project_b64`, `remote_b64`: base64url(no padding)

API 경계:

- `DELETE /api/admin/sessions/{id}`
- Header: `X-OpenSession-Admin-Key`
