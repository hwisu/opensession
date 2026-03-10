# OpenSession Desktop (Preview)

[English](README.md)

이 패키지는 `../web`의 기존 Svelte UI를 재사용하는 데스크톱 셸입니다.

## 실행 (dev)

```bash
cd desktop
npm install
npm run dev
```

`npm run dev`는 Tauri와 웹 UI 개발 서버를 함께 시작합니다. `opensession-server`는 필요하지 않습니다.

데스크톱 명령을 실행하기 전에는 `mise`로 저장소 툴체인을 설치하세요.

```bash
mise install
```

## 빌드

```bash
cd desktop
npm run build
```

빌드 흐름:

1. `web` 정적 번들 빌드 (`../web/build`)
2. Tauri 데스크톱 번들

macOS universal 번들(서명 없는 로컬 검증):

```bash
npm run tauri:build -- --target universal-apple-darwin --bundles app --no-sign --ci
```

## 참고

- UI 컴포넌트는 기존 `web` 앱을 통해 `@opensession/ui`에서 재사용합니다.
- 데스크톱 런타임에서는 세션/권한/인증 조회가 로컬 DB와 git-native 저장소를 사용하는 Tauri 명령으로 처리됩니다.
- 선택 사항: `OPENSESSION_LOCAL_DB_PATH`로 사용자 지정 sqlite 파일 경로를 지정할 수 있습니다.

## 런타임 설정 (Desktop Local)

Desktop local runtime은 typed summary model 기반 런타임 설정을 제공합니다.

- `summary.provider.id|endpoint|model`
- `summary.prompt.template`
- `summary.response.style|shape`
- `summary.storage.trigger|backend`
- `summary.source_mode`

Desktop local 정책:

- `auth_enabled=false`이면 account/auth 섹션을 숨깁니다. 기본 데스크톱 로컬 동작입니다.
- source mode 선택기는 숨겨지며 내부적으로 `session_only`로 고정됩니다.
- summary storage backend 기본값은 `hidden_ref`입니다.
- `hidden_ref` 모드에서도 검색/필터 성능을 위해 로컬 SQLite(`local.db`)에 searchable list metadata를 기록합니다.
- response preview는 결정론적 fixture 렌더링이며 LLM/network dry-run이 아닙니다.

프로바이더별 표시 필드:

- `ollama` (`http`): endpoint + model
- `codex_exec`, `claude_cli` (`cli`): binary status + model
- `disabled`: provider detail 필드 숨김

Desktop local 문서:

- `/docs`는 `opensession-server` 없이도 로컬 IPC(`desktop_get_docs_markdown`)로 렌더링할 수 있습니다.

Desktop vector search (선택 기능):

- vector ranking은 세션 전체 문자열이 아니라 event/line chunk 단위입니다.
- `vector_search` 설정은 typed payload로 저장됩니다.
- 기본 모델은 로컬 Ollama(`http://127.0.0.1:11434`)의 `bge-m3`입니다.
- 모델 설치는 Settings의 `desktop_vector_install_model`에서 명시적으로 실행하며 preflight 상태로 진행률을 확인할 수 있습니다.
- 인덱싱은 `desktop_vector_index_rebuild`로 명시적으로 실행하고 `desktop_vector_index_status`로 상태를 조회합니다.
- hidden refs는 summary ledger 저장소로 유지되고, vector/list metadata는 질의 성능을 위해 로컬 SQLite(`local.db`)에 유지됩니다.

## 릴리스

- 제품 버전은 workspace `Cargo.toml`에서 `scripts/sync-product-version.mjs`로 desktop 파일에 동기화됩니다.
- 릴리스 전에 `node scripts/sync-product-version.mjs --check`를 실행하고, 적용이 필요하면 `--write`를 사용하세요.
- GitHub Actions `Release` 워크플로(수동)는 다음을 실행합니다.
  1. `release-plz update` + 릴리스 publish
  2. macOS universal Tauri 번들 빌드 (`.dmg`, `.app.zip`, checksum)
  3. 태그 `v<workspace-version>`에 아티팩트 업로드
- universal 정책: 릴리스 빌드는 `universal-apple-darwin`을 사용하고 `lipo -archs` 결과가 `x86_64 arm64`인지 검증합니다.
- 보안 게이트: 코드 서명 + notarization 검증이 통과한 경우에만 desktop 아티팩트를 업로드합니다.
  필요한 저장소 시크릿:
  - `APPLE_CERTIFICATE`
  - `APPLE_CERTIFICATE_PASSWORD`
  - `APPLE_SIGNING_IDENTITY`
  - `APPLE_ID`
  - `APPLE_PASSWORD`
  - `APPLE_TEAM_ID`
- release/CI/local 사전 점검 도우미:
  - `node scripts/validate/desktop-build-preflight.mjs --mode release --os macos`
