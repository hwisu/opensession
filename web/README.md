# OpenSession Web

[한국어](README.ko.md)

This package contains the Svelte web app for OpenSession.

## Run

```bash
npm ci
npm run dev
```

## Validate

```bash
npm run check
```

## Live E2E

Requires local worker (`wrangler dev`) and server (`opensession-server`) to already be running.

```bash
OPENSESSION_E2E_WORKER_BASE_URL=http://127.0.0.1:8788 \
OPENSESSION_E2E_SERVER_BASE_URL=http://127.0.0.1:3000 \
OPENSESSION_E2E_ALLOW_REMOTE=0 \
CI=1 \
npm run test:e2e:live -- --reporter=list
```

For product-level usage and install-and-forget flow, see the root docs:

- `../README.md`
- `../README.ko.md`
- `../docs.md`
- `../docs.ko.md`
- `../docs/development-validation-flow.md`
