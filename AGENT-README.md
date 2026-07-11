# WDAPM Agent Guide

## Architecture

WDAPM is a Rust workspace with a React administration frontend.

- `crates/app`: process entrypoint and service wiring.
- `crates/gateway`: public provider proxy routes, Platform API Key authentication, retries, and request logging.
- `crates/providers/*`: provider-specific route, upstream URL, authentication, and response behavior.
- `crates/admin-api`: administration endpoints.
- `crates/storage`: SQLite persistence and credential encryption.
- `crates/scheduler`: provider account and egress proxy selection.
- `crates/web/app`: React administration UI.

## Authentication Boundary

Client credentials always contain a WDAPM Platform API Key. The gateway accepts provider-compatible formats, sanitizes them before creating `RequestEnvelope`, validates the Platform API Key, and then lets the selected provider adapter inject the upstream account credential.

| Route | Accepted client format | Upstream format |
| --- | --- | --- |
| `/exa/*` | Bearer or `x-api-key` | `x-api-key` |
| `/tavily/*` | Bearer or JSON `api_key` | Bearer and replaced body key when present |
| `/firecrawl/*` | Bearer | Bearer or keyless |
| `/jina/*` | Bearer | Bearer or keyless Reader |

Never log or forward a Platform API Key. Tavily body authentication is represented in the sanitized request body as an empty `api_key`; the adapter replaces it for every selected account attempt.

## Provider Accounts

Provider account credentials are encrypted in `provider_account_credentials`. Provider-specific configuration uses the `provider_accounts.config` JSON column. Jina stores optional `reader_base_url` and `search_base_url` values there. Its legacy generic `base_url` is used only as a Reader fallback.

The admin API can replace provider credentials but must not reveal them. Audit records may state that a credential changed but must not contain either key value.

## Environment and Startup

Use `.env.example` as the configuration reference. Secrets belong in `.env`, which is ignored. Start the complete service with:

```bash
docker compose up -d
```

## Verification

```bash
rtk cargo fmt --all -- --check
rtk cargo test --workspace
rtk cargo clippy --workspace --all-targets -- -D warnings
cd crates/web/app
rtk bun run lint
rtk bun run build
```

Provider tests must cover route-specific authentication and URL selection. Gateway tests must cover credential conflicts and sanitization. Use mock upstreams for end-to-end verification; real provider credentials are not required.
