# copilot-gateway

A small Rust service that exposes your **GitHub Copilot** models through an
**Anthropic-compatible API** (`/v1/messages`), so the **Claude Code** CLI (and
other Anthropic clients) can talk to Copilot models.

It logs in to GitHub with the OAuth **device authorization flow**, exchanges that
for a short-lived Copilot token (auto-refreshed), and translates between the
Anthropic Messages API and Copilot's OpenAI-style chat API — including streaming
(SSE), tool calls, images, and token counting.

```
Claude Code  ──Anthropic /v1/messages──▶  copilot-gateway  ──OpenAI /chat/completions──▶  GitHub Copilot
            ◀──── SSE / JSON ───────────                   ◀──── SSE / JSON ───────────
```

## Requirements

- Rust toolchain (`cargo`), 2021 edition.
- A GitHub account with Copilot access.

## Build

```powershell
cargo build --release
# binary: target\release\copilot-gateway.exe
```

## Usage

### 1. Authenticate (one time)

```powershell
.\target\release\copilot-gateway.exe auth
```

It prints a URL and a user code. Open the URL in your browser, enter the code,
and approve. The GitHub token is saved to `~/.copilot-gateway/github_token`
(`%USERPROFILE%\.copilot-gateway\github_token` on Windows).

> `start` will automatically launch the device flow if no token is found, so
> this step is optional.

### 2. Start the gateway

```powershell
.\target\release\copilot-gateway.exe start --port 4141
```

Options:

| Flag             | Default       | Description                                   |
| ---------------- | ------------- | --------------------------------------------- |
| `--port`         | `4141`        | Port to listen on.                            |
| `--host`         | `127.0.0.1`   | Bind address.                                 |
| `--account-type` | `individual`  | `individual`, or your org/enterprise slug.    |

### 3. Point Claude Code at it

Set the Anthropic base URL to the gateway and run Claude Code:

```powershell
$env:ANTHROPIC_BASE_URL = "http://127.0.0.1:4141"
$env:ANTHROPIC_API_KEY  = "dummy"   # any non-empty value; the gateway ignores it
claude
```

Pick a model that your Copilot account exposes (see `GET /v1/models`). For
example, set `ANTHROPIC_MODEL` to a Copilot model id.

## Model selection & aliasing

The gateway forwards the request's `model` to Copilot, but first resolves it so
the Claude Code `/model` picker works even though it sends Anthropic-style ids:

1. **User override** — exact or family key in `~/.copilot-gateway/model_map.json`.
2. **Pass-through** — if the id is already a valid Copilot id (e.g. you typed
   `/model claude-opus-4.7`), it is used as-is.
3. **Family normalization** — any id containing `opus` / `sonnet` / `haiku`
   (e.g. `claude-opus-4-1-20250805` from the picker) maps to a current Copilot
   id: `claude-opus-4.8` / `claude-sonnet-4.6` / `claude-haiku-4.5`.
4. **Unknown** — anything else is passed through unchanged.

Optional `~/.copilot-gateway/model_map.json` (highest priority). Keys are
lowercased; use a family word or an exact id:

```json
{
  "opus": "claude-opus-4.7",
  "claude-3-5-sonnet-20241022": "claude-sonnet-4.6"
}
```

Mappings are logged (`Mapped model 'X' -> 'Y'`).

## API

| Method | Path                        | Description                                  |
| ------ | --------------------------- | -------------------------------------------- |
| `GET`  | `/`                         | Health check.                                |
| `POST` | `/v1/messages`              | Anthropic Messages API (streaming + JSON).   |
| `POST` | `/v1/messages/count_tokens` | Approximate input token count.               |
| `GET`  | `/v1/models`                | List available Copilot models.               |

### Quick check (models)

```powershell
curl http://127.0.0.1:4141/v1/models
```

## How it works

- **Auth** (`src/auth`, `src/github/copilot_token.rs`): device flow → GitHub
  token → `copilot_internal/v2/token` → short-lived Copilot token, refreshed in a
  background task using `refresh_in`. If the GitHub token is rejected (HTTP 401),
  the log prints a hint to re-run `copilot-gateway auth`.
- **Headers** (`src/github/headers.rs`): replicates the headers the official
  Copilot Chat client sends (integration id, editor version, request id, etc.).
- **Model resolution** (`src/model_map.rs`): maps the client-requested model to a
  valid Copilot id (user override → pass-through → family normalization → as-is).
- **Translation** (`src/translate`):
  - `request.rs` — Anthropic request → OpenAI request (system prompt, text,
    images, `tool_use`/`tool_result`, `tools`, `tool_choice`, `thinking`).
  - `response.rs` — non-streaming OpenAI response → Anthropic message.
  - `stream.rs` — OpenAI SSE chunks → Anthropic SSE event state machine
    (`message_start` → `content_block_*` → `message_delta` → `message_stop`).

## Development

```powershell
cargo test          # run unit/integration tests
cargo clippy        # lints
cargo build --release
```

Configuration constants (Copilot/VS Code versions, GitHub client id) live in
`src/config.rs`. If the upstream rejects requests, bump `VSCODE_VERSION` /
`COPILOT_VERSION` there.

## Notes / Limitations

- The Copilot internal API is not officially documented; header values may need
  occasional updates.
- `count_tokens` uses a `cl100k_base` approximation, not the exact tokenizer of
  every model.
- The saved GitHub token is sensitive — it is stored with `0600` permissions on
  Unix. Do not commit it.
- The GitHub token is long-lived but can be revoked. On an upstream `401` the
  gateway logs a hint to re-run `copilot-gateway auth`.

## License

MIT
