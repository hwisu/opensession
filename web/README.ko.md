# OpenSession Web

[English](README.md)

이 패키지는 OpenSession용 Svelte 웹 앱을 포함합니다.

## 실행

```bash
npm ci
npm run dev
```

## 검증

```bash
npm run check
```

## Live E2E

로컬 worker(`wrangler dev`)와 서버(`opensession-server`)가 이미 실행 중이어야 합니다.

```bash
OPENSESSION_E2E_WORKER_BASE_URL=http://127.0.0.1:8788 \
OPENSESSION_E2E_SERVER_BASE_URL=http://127.0.0.1:3000 \
OPENSESSION_E2E_ALLOW_REMOTE=0 \
CI=1 \
npm run test:e2e:live -- --reporter=list
```

제품 수준 사용법과 install-and-forget 흐름은 루트 문서를 참고하세요.

- `../README.md`
- `../README.ko.md`
- `../docs.md`
- `../docs.ko.md`
- `../docs/development-validation-flow.md`
