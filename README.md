# OpenCode Monitor

A macOS desktop app for monitoring and interacting with [OpenCode](https://github.com/sst/opencode) agents across multiple workspaces.

Forked from [CodexMonitor](https://github.com/Dimillian/CodexMonitor) by Dimillian, adapted to use OpenCode's REST API + SSE backend while preserving CodexMonitor frontend event compatibility.

## Status

**Active development** — the REST/SSE backend is live for thread/session lifecycle, event translation, messaging, model discovery, and image attachments. Remaining work focuses on feature-parity polish and OpenCode-specific UX cleanup.

## Architecture

- **Frontend**: React 19 + Vite + TypeScript
- **Backend**: Tauri 2 (Rust) — runs against `opencode serve` (HTTP REST + SSE)
- **Protocol**: OpenCode REST API + SSE, translated in Rust to CodexMonitor-shaped frontend events

## Development

```bash
npm install
npm run tauri:dev
```

### Validation

```bash
npm run typecheck
npm run test
cd src-tauri && cargo check
cd src-tauri && cargo test
```

## License

MIT — see [LICENSE](LICENSE)
