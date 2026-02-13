# OpenCode Monitor

A macOS desktop app for monitoring and interacting with [OpenCode](https://github.com/sst/opencode) agents across multiple workspaces.

Forked from [CodexMonitor](https://github.com/Dimillian/CodexMonitor) by Dimillian, adapted to use OpenCode's ACP (Agent Communication Protocol) instead of the Codex app-server protocol.

## Status

**Active development** — ACP backend migration is live for thread/session lifecycle, event translation, and model discovery. Remaining work focuses on parity polish and UX cleanup.

## Architecture

- **Frontend**: React 19 + Vite + TypeScript
- **Backend**: Tauri 2 (Rust) — spawns `opencode acp` child processes via stdio JSON-RPC
- **Protocol**: OpenCode ACP v1 (nd-JSON-RPC over stdio)

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
