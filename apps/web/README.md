# Litter Web

Litter Web is the PWA/WASM entrypoint.

Status: first scaffold. The Rust `codex-web-client` crate owns the browser-side model, JSON-RPC request effects, response reduction, and Alleycat-over-iroh WASM transport. The JavaScript shell renders state and executes effects.

## Build

```bash
make web-check
make web-wasm
make web-serve
```

`make web-wasm` requires `wasm-pack`.

The Alleycat transport pulls `iroh` into the browser build. `iroh` uses `ring` for TLS, so the wasm build also needs a wasm-capable `clang`. Set `WASM_CC=/path/to/clang` when `clang` is not on `PATH`.

## Manual Browser Run

Start the web app:

```bash
make web-serve
```

Open `http://127.0.0.1:8080/`.

Use the attach fork:

1. Click `Pair with kittylitter`.
2. Start or reuse a host daemon:

   ```bash
   cargo run --manifest-path services/kittylitter/Cargo.toml -- serve
   ```

3. In another terminal, print a pair payload:

   ```bash
   cargo run --manifest-path services/kittylitter/Cargo.toml -- pair
   ```

4. Paste the JSON payload into the browser.
5. Keep `Agent` set to `codex`.
6. Click `Pair`.

Use `Codex WebSocket URL` only for a browser-compatible endpoint. A raw local Codex app server URL such as `ws://127.0.0.1:8390` works only when the browser can reach that exact host and the endpoint accepts browser `Origin` headers.

## Real-Browser E2E

Run the PWA against the visible Chromium CDP endpoint used by the browser tools:

```bash
make web-e2e-real-browser
```

Defaults:

- `RESEARCH_BROWSER_URL=http://localhost:9222`
- `LITTER_WEB_URL=http://127.0.0.1:8080/`
- `LITTER_ALLEYCAT_AGENT=codex`
- `LITTER_WEB_E2E_TIMEOUT_MS=30000`

The test is read-only by default. It loads the app, connects through Alleycat when `LITTER_ALLEYCAT_PAIR_PAYLOAD` is set, waits for threads, opens the first thread, and writes screenshots plus `summary.json` under `artifacts/web-e2e/<timestamp>/`.

Alleycat run:

```bash
LITTER_ALLEYCAT_PAIR_PAYLOAD='{"v":1,"node_id":"...","token":"...","relay":"..."}' \
LITTER_ALLEYCAT_AGENT=codex \
make web-e2e-real-browser
```

Direct WebSocket fallback:

```bash
LITTER_WEB_TRANSPORT=websocket \
LITTER_CODEX_WS_URL=ws://127.0.0.1:<browser-compatible-port>/rpc \
make web-e2e-real-browser
```

Optional mutating send-turn path:

```bash
LITTER_WEB_E2E_SEND_TURN=1 \
LITTER_WEB_E2E_MESSAGE='E2E smoke test from Litter Web.' \
make web-e2e-real-browser
```

Set `LITTER_WEB_E2E_KEEP_TAB=1` to leave the browser tab open after the run.

## Browser Transport Constraints

Browser `WebSocket` always sends an `Origin` header, and the current Codex app-server rejects requests that include `Origin`. Browser `WebSocket` also cannot set an `Authorization` header.

Use one of these for v1:

- Alleycat pair payload in the PWA. This is the primary browser path.
- `wss://...` behind a reverse proxy that strips or handles `Origin` and injects `Authorization: Bearer <token>` before forwarding to the Codex app server.
- A browser-compatible local WebSocket endpoint that does not enforce the Codex app-server `Origin` rule.

Do not add token query parameters unless the upstream server explicitly supports them.
