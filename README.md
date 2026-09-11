# Fujin 風神 — actions layer of Meisei

> **Meisei** 明晰 (“clarity”) is an open pipeline that carries raw intent through
> understanding → decision → plan → action to a finished result.

[![Meisei](https://img.shields.io/badge/meisei-明晰-1f2937.svg)](https://meisei.ru)
[![License: Apache-2.0 WITH Commons-Clause](https://img.shields.io/badge/license-Apache--2.0%20WITH%20Commons--Clause-blue.svg)](LICENSE)

<sub>
torii · satori · enma · yatagarasu · <b>fujin</b> · daruma
&nbsp;—&nbsp; intake · sensemaking · decisions · planning · <b>actions</b> · execution (terminal)
</sub>

## What it is

Fujin is the **actions** layer of the Meisei pipeline: the maturity boundary
between deliberation and execution. Its `pack_ai` operation builds a typed
`ActionPacket` (work order, execution steps, gates, target files, required
documents, handoff projects/tasks), and its deterministic maturity check
(`maturity::assess`, with strictness levels)
decides whether a packet is ripe for handoff — only a mature packet may cross
into daruma as tasks/plans. Invalid model arguments get one schema-aware retry;
a second invalid result fails closed. Domain primitives stay storage-agnostic; the server
persists action packets. The crate has no dependency on daruma or sibling
layers; concrete daruma adapters live inside the host.

`pack_ai` writes the optional execution envelope (`target_files`,
`conflict_policy`, `required_checks`, `reviewer_profile`). Explicit PlanBrief
bounds take precedence over model output and malformed bounds fail validation.
Unspecified bounds remain empty; these declarations do not enforce filesystem
access. The unused standalone minimality validator has been removed: maturity
does not claim to verify implementation simplicity or test results.

## Repository layout

- `src/` — the `fujin` library: `ActionPacket`/handoff types, `pack_ai`,
  deterministic maturity assessment and error types.
- `server/` — `fujin-server`, a thin, independently-deployed HTTP/MCP wrapper over
  the library (the axum/tokio scaffold comes from [`layer-kit`](../layer-kit)).
- `deploy/` — release `build.sh` (stamps the git SHA into `/healthz`) and a
  systemd user unit.

## Build & run

```sh
cargo run -p fujin-server
# GET  /healthz   — open liveness/version probe
# POST /v1/mcp    — platform-token gated MCP surface:
#                   fujin.pack, fujin.assess, fujin.list /
#                   fujin.list_packets, fujin.get / fujin.get_packet
```

For production builds use `deploy/build.sh` so `/healthz` reports the real git SHA
instead of `"dev"`.

## Configuration (env)

| Variable | Default | Purpose |
| --- | --- | --- |
| `FUJIN_PORT` | `8094` | HTTP listen port |
| `FUJIN_PLATFORM_SECRET` | unset | HMAC key; if unset, `/v1/mcp` is closed |
| `FUJIN_VERSION` | crate version | Version reported by `/healthz` |
| `FUJIN_DB` | `./fujin.db` | SQLite store path (`layer_kit::store::Store`) |
| `OPENAI_API_KEY` | unset | Optional AI fallback provider for `fujin.pack`; without a key the method answers 503 `ai_not_configured` |
| `OPENAI_BASE_URL` | `https://api.openai.com/v1` | Base URL of the OpenAI-compatible API |
| `OPENAI_MODEL` | `gpt-4.1` | Model used by the AI fallback |

## Docs

Pipeline canon and layer contracts: https://meisei.ru/docs

## License

Apache-2.0 WITH Commons-Clause — see [LICENSE](LICENSE) and
[LICENSE.commons-clause.md](LICENSE.commons-clause.md).
