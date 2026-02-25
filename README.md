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

## Credits & Support

OpenCodeMonitor is a fork of [CodexMonitor](https://github.com/Dimillian/CodexMonitor) by [Thomas Ricouard](https://github.com/Dimillian). The majority of this app's functionality comes from his excellent work.

**Support the original author:**
- [Sponsor Thomas on GitHub](https://github.com/sponsors/Dimillian)
- [Ice Cubes for Mastodon](https://apps.apple.com/app/ice-cubes-for-mastodon/id6444915884) — his open-source Mastodon client

**Support this fork:**
- [Buy me a coffee](https://buymeacoffee.com/jacobjmc)

## License

MIT — see [LICENSE](LICENSE)
