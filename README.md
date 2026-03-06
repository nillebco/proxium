# Proxium - Universal Authentication Proxy

A self-hosted universal proxy in Rust that sits between internal users/agents and external services (LLMs, SaaS APIs, databases, etc.), handling authentication, secret injection, audit logging, and ephemeral container lifecycle.

## Why This Exists

Your API keys are yours: don't give them to an agent.

Proxium resolves two questions per request:
1. **Who is the caller?** → Identity (user, node, group, token)
2. **Where do they want to go?** → Destination (hostname → upstream URL + API key)

```
(Identity, Destination) → allow/deny → inject secret → forward → audit log
```

## Quick Start

### Prerequisites

- [`lr`](https://github.com/nillebco/localhost-router) - Localhost router for HTTPS
- [`fnox`](https://github.com/jdx/fnox) - Secret injection at runtime

### Setup

```sh
# Copy and configure fnox.toml (requires rbw, bitwarden-cli, or any supported secrets manager)
cp fnox.example.toml fnox.toml

# Copy and configure your services
cp config.example.toml config.toml

# Set up hostname forwarding (one-time setup; you can forward multiple hosts to the same port)
lr add openai 8123
lr add claude 8123

# Run with fnox to inject secrets at startup
fnox exec proxium serve
```

### Test It

In a different terminal:

```sh
# Quick health check (this is what your agents will do)
curl -skv https://openai.localhost/v1/models

# Use with tools that accept BASE_URL
OPENAI_BASE_URL=https://openai.localhost ANTHROPIC_BASE_URL=https://claude.localhost claude
```

## CLI Reference

### `proxium serve [config.toml]`

Start the proxy server. Defaults to `config.toml` in the current directory.

```sh
proxium serve
proxium serve /etc/proxium/config.toml
```

### `proxium keys`

Manage API keys (used when `deployment.mode = "apikey"`).

All subcommands accept `--keys-file <path>` to override the default store location (`api_keys.json`).

#### `proxium keys add <user_id> [--expires <ISO_DATE>]`

Create a new API key for a user. Returns the key ID and secret (the secret is shown only once).

```sh
proxium keys add alice
# key_id: key_3f8a1c2d4e5b6f7a
# secret: pk_Xy9zAbCdEfGhIjKlMnOpQrStUvWxYz01234567

proxium keys add alice --expires 2026-12-31T23:59:59Z
proxium keys add ci-bot --keys-file /var/lib/proxium/api_keys.json
```

#### `proxium keys list [--keys-file <path>]`

List all API keys with their user ID and expiry date.

```sh
proxium keys list
# key_id                              user_id               expires_at
# ------------------------------------------------------------------------
# key_3f8a1c2d4e5b6f7a               alice                 never
# key_9b7c6d5e4f3a2b1c               ci-bot                2026-12-31T23:59:59+00:00
```

#### `proxium keys revoke <key_id> [--keys-file <path>]`

Revoke an API key by its ID.

```sh
proxium keys revoke key_3f8a1c2d4e5b6f7a
# OK
```

## Configuration

### Deployment Modes

| Mode | Identity Source | Use Case |
|------|-----------------|---------|
| `none` | No auth required | Local testing |
| `tailscale` | Tailscale `whois` on source IP | Internal network |
| `oidc` | JWT validated against JWKS endpoint | Enterprise SSO |
| `apikey` | `X-Api-Key` header lookup | Service-to-service |

### Server Configuration

```toml
[deployment]
mode = "none"  # "tailscale", "oidc", "apikey", "none"
# Required when mode = "apikey"; defaults to api_keys.json
key_store_path = "api_keys.json"

[server]
listen = "0.0.0.0:8123"
```

When `mode = "apikey"`, callers must include an `X-Api-Key: <secret>` header. Keys are managed with `proxium keys add/list/revoke` and persisted to `key_store_path`.

### Service Definitions

#### Static Service (always running upstream)

```toml
[[services]]
hostname = "openai.internal.domain.com"
upstream = "https://api.openai.com"
api_key_ref = "env://OPENAI_API_KEY"
inject_header = "Authorization"
inject_prefix = "Bearer "
ephemeral = false
```

#### Ephemeral Service (container managed by proxy)

```toml
[[services]]
hostname = "myagent.internal.domain.com"
upstream = "http://localhost:9090"
api_key_ref = "env://AGENT_API_KEY"
ephemeral = true
container_image = "my-agent:latest"
idle_timeout_secs = 300
```

Ephemeral services follow this lifecycle:
- `NONE` → `STARTING` → `READY` → `IDLE` → `STOPPED`
- Container starts on first request, stops after idle timeout
- Concurrent requests during startup are queued and flushed

## Architecture

### Deployment Trait

The core abstraction captures identity and destination resolution:

```rust
trait Deployment {
    async fn resolve(&self, req: &Request) -> Result<(Identity, Destination)>;
}
```

### Technology Stack

| Component | Crate |
|-----------|-------|
| HTTP server | `axum` |
| Reverse proxy | `hyper`, `hyper-util` |
| Middleware | `tower` |
| Container management | `bollard` |
| TLS | `hyper-tls`, `rustls` |
| Async runtime | `tokio` |
| Logging | `tracing`, `tracing-subscriber` |

## Use Cases

### LLM Gateway

Route multiple LLM providers through a single secure entry point:

```toml
[[services]]
hostname = "openai.llm.local"
upstream = "https://api.openai.com"
api_key_ref = "env://OPENAI_API_KEY"
inject_header = "Authorization"
inject_prefix = "Bearer "

[[services]]
hostname = "anthropic.llm.local"
upstream = "https://api.anthropic.com"
api_key_ref = "env://ANTHROPIC_API_KEY"
```

### Internal API Gateway

Expose internal services with authentication:

```toml
[[services]]
hostname = "db.internal.local"
upstream = "http://postgres:5432"
ephemeral = false
```

### Ephemeral Agent Runner

Spin up containers on-demand for AI agents:

```toml
[[services]]
hostname = "agent.llm.local"
upstream = "http://localhost:8080"
ephemeral = true
container_image = "nilleb/ai-agent:latest"
idle_timeout_secs = 600
```

## Comparison with Tailscale Aperture

| Feature | Proxium | Tailscale Aperture |
|---------|---------|-------------------|
| Protocol | Universal (any HTTP) | LLM-specific |
| Hosting | Self-hosted | Managed by Tailscale |
| Source | Open source | Closed source |
| Extensibility | Full control | Limited |

## License

MIT
