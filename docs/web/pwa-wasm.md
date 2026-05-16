# PWA/WASM Implementation Notes

## Decision

Add a web shell that compiles a Litter-owned Rust client core to WASM.

Do not edit `shared/third_party/codex/`.

## Current Slice

- `shared/rust-bridge/codex-web-client` adds a WASM-safe TEA-style core.
- `apps/web` adds the installable web shell, manifest, service worker, and static assets.
- The JavaScript shell executes effects from Rust: open Alleycat, open WebSocket, send JSON-RPC, and persist config.

## Transport Decision

Use Alleycat as the primary PWA transport.

Reason:

- Alleycat already gives Litter an iroh-based pair payload and token flow.
- `iroh` has browser/WASM support.
- Direct browser `WebSocket` cannot match the native app-server path because browsers always send `Origin`, and the current Codex app-server rejects requests that include `Origin`.
- Browser `WebSocket` also cannot set bearer auth headers.

Keep direct WebSocket as a fallback for browser-compatible proxies or local test servers.

## Build Requirement

The browser build needs `clang` for `iroh` + `ring` on `wasm32-unknown-unknown`.

Do not fall back to `cc` or `gcc`. They can produce a `.wasm` package with unresolved bare `env` imports that compiles but fails to load in the browser.

## Next Work

- Move shared conversation render projections from `codex-mobile-client` into a WASM-safe crate.
- Run the Alleycat-backed real-browser E2E path on a host with wasm-capable `clang`.
- Add mock JSON-RPC tests for reducer/render behavior.
- Add a documented reverse-proxy target only if direct WebSocket remains useful.
