# Sori

A Rust service that forwards HTTP notifications to Discord webhooks.
Register destinations in YAML and select where to send each message using the request's `destination` field.

## Features

- Configure the listening address, port, and webhook destinations in YAML
- Optionally authenticate notification requests with a Bearer token
- Set a display name for each request with `bot_name`
- Return the Discord message ID after delivery is confirmed
- Check service availability through `/health`
- Disable automatic user, role, and everyone mention notifications

## Getting started

Install Git, a stable Rust toolchain, and the native build tools required by your platform.
See the [Rust installation guide](https://rust-lang.org/tools/install/) for setup instructions.
The commands below use a POSIX-compatible shell.

### 1. Build

```sh
git clone https://github.com/russell-yoo/sori.git
cd sori
cargo build --release --locked
```

### 2. Configure

```sh
cp config.example.yaml config.yaml
chmod 600 config.yaml
```

Replace the placeholder webhook URLs in `config.yaml` with your Discord webhook URLs.
To use authentication, replace `auth.api_token` with a long, randomly generated token. To disable authentication, omit the `auth` section.

Local files named `config.yaml` or `config.*.yaml` are ignored by Git. The tracked `config.example.yaml` contains placeholder values only.

### 3. Run

```sh
./target/release/sori --config ./config.yaml
```

When `--config` is omitted, Sori reads `config.yaml` from the current working directory.

The default listening address is `127.0.0.1:44334`. Check the service from another terminal on the same host:

```sh
curl -i http://127.0.0.1:44334/health
```

Adjust the URL if you change the server address or port. A healthy instance returns HTTP `200` with the body `ok`.

## Configuration

Application settings are loaded from a single YAML file:

```yaml
server:
  host: "127.0.0.1"
  port: 44334

auth:
  api_token: "replace-with-a-long-random-token"

destinations:
  - name: news
    web_hook_url: "https://discord.com/api/webhooks/WEBHOOK_ID/WEBHOOK_TOKEN"
```

### Server

| Field | Default | Description |
| --- | --- | --- |
| `server.host` | `127.0.0.1` | The IPv4 or IPv6 address to bind to |
| `server.port` | `44334` | The TCP port, from `0` to `65535` |

Omitting `server` uses both defaults. Omitting an individual field uses that field's default.
Use `127.0.0.1` for access from the same host or `0.0.0.0` to listen on all IPv4 interfaces.
Port `0` lets the operating system select an available port. Sori prints the actual listening address at startup.

### Authentication

| Configuration | Behavior |
| --- | --- |
| `auth` omitted | Authentication is disabled |
| `auth.api_token` omitted or set to `null` | Authentication is disabled |
| A non-empty token is configured | Notification requests require a matching Bearer token |
| An empty or whitespace-only token is configured | Startup fails |

When authentication is disabled, `/notifications` accepts requests without an `Authorization` header.
The `/health` endpoint is always accessible without authentication.

### Destinations

| Field | Description |
| --- | --- |
| `destinations` | A list containing at least one destination |
| `name` | A unique, case-sensitive name matched by the request's `destination` field. Leading and trailing whitespace are not allowed. |
| `web_hook_url` | The destination's absolute HTTP or HTTPS webhook URL |

The destination `name` selects a webhook. The request's `bot_name` controls the sender name displayed in Discord.

### Loading and validation

Configuration is loaded once at startup. Restart Sori to apply changes.
Startup fails for unreadable files, invalid YAML, unknown fields, missing required fields, invalid server settings, empty tokens, empty destination lists, invalid destination names, or invalid webhook URLs.
YAML parsing errors report the line and column when available without including input values.

### Command-line options

| Option | Description |
| --- | --- |
| `--config <FILE>` | Path to the YAML configuration file. Defaults to `config.yaml`. |
| `-h`, `--help` | Print usage information and exit |

Relative paths are resolved from the process's working directory. When running under a service manager, pass an absolute path with `--config`.

```sh
./target/release/sori --config /absolute/path/to/config.yaml
```

## API

### `GET /health`

Checks whether the service can respond to HTTP requests. Authentication is not required. This endpoint does not check connectivity to Discord.

```text
ok
```

### `POST /notifications`

Send a JSON request body with `Content-Type: application/json`.
Include `Authorization` only when authentication is enabled:

```http
Authorization: Bearer YOUR_API_TOKEN
Content-Type: application/json
```

Request body:

| Field | Required | Description |
| --- | --- | --- |
| `message` | Yes | The message to send |
| `destination` | Yes | A destination `name` registered in YAML |
| `bot_name` | No | The sender name displayed in Discord. Defaults to `Sori` when omitted or set to `null`. |

```json
{
  "message": "Deployment completed.",
  "destination": "news",
  "bot_name": "Deploy"
}
```

Send a notification to the `news` destination from the example configuration.
Replace `YOUR_API_TOKEN` with the token in your YAML configuration, or omit the `Authorization` header when authentication is disabled.

```sh
curl --connect-timeout 5 --max-time 20 \
  http://127.0.0.1:44334/notifications \
  -H 'Authorization: Bearer YOUR_API_TOKEN' \
  -H 'Content-Type: application/json' \
  --data '{"message":"Deployment completed.","destination":"news","bot_name":"Deploy"}'
```

Use the appropriate server address and port if you changed the defaults.

Success response:

```json
{
  "message_id": "123456789012345678",
  "destination": "news"
}
```

Application errors use the following response format:

```json
{"error":"Invalid or Missing API token"}
```

| Status | Meaning |
| --- | --- |
| `200` | Delivery confirmed |
| `400` | Unknown destination |
| `401` | Missing or invalid API token when authentication is enabled |
| `502` | Discord delivery could not be confirmed |
| `504` | The Discord request timed out |

Malformed JSON, missing required fields, and similar input errors are rejected by Axum's JSON extractor and may use a different error response format.

## Deployment

Sori serves HTTP. For HTTPS access, use a reverse proxy or load balancer
that terminates TLS.

Configure process supervision, DNS, and network access according to
your deployment environment.

## Behavior and limitations

- Clients must split long messages before sending them. Discord webhook `content` is limited to 2,000 characters per request. See the [Discord webhook documentation](https://docs.discord.com/developers/resources/webhook#execute-webhook).
- Discord requests time out after 10 seconds and are not retried automatically.
- Discord HTTP errors, including `429`, currently produce a `502` response.
- A timed-out request may still have delivered the message. Retrying it can create duplicates.

## Development

```sh
cargo fmt --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
```
