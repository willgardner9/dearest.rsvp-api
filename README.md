# dearest

A basic Rust web API.

## Stack

- Rust (edition 2024), tokio, axum
- Postgres 18 with SeaORM
- SeaORM migrations in the `migration` crate
- `tower-http` request tracing
- Docker Compose to run the full stack

## Start the stack

```bash
docker compose up --build
```

The API listens on port 3000. To use a different host port, set `API_PORT`:

```bash
API_PORT=3001 docker compose up --build
```

Check that the API runs:

```bash
curl -i localhost:3000/health
```

The endpoint returns `200` and `{"status":"ok","db":"up"}` when the database is reachable. It returns `503` when the database is down.

Docker Compose also starts Mailpit, a local email server. The API sends email to it. Open `http://localhost:8025` to read the email, for example the verification email after a signup.

## Auth API

All bodies are JSON. An error has the body `{"error": "<code>"}`. A session cookie (`dearest_session`) identifies the user. A user must verify the email before the routes that are marked "Verified" work.

| Method and path | Who | Purpose |
| --- | --- | --- |
| `POST /auth/signup` | anyone | Create an account and a session. Send the verification email. |
| `POST /auth/login` | anyone | Start a session. |
| `POST /auth/logout` | anyone | End the session. |
| `GET /auth/me` | session | The user and the tenants of the user. |
| `POST /auth/verify-email` | anyone | Verify the email with a token. |
| `POST /auth/verify-email/resend` | session | Send a new verification email. |
| `POST /auth/password-reset` | anyone | Send a reset email. Always `202`. |
| `POST /auth/password-reset/confirm` | anyone | Set a new password with a token. Ends all sessions. |
| `GET /invites/{token}` | anyone | Show the tenant and the inviter of an invite. |
| `POST /invites/{token}/accept` | anyone | Accept an invite. A new user sends `name` and `password`. |
| `POST /tenants` | Verified | Create a tenant. |
| `GET /tenants/{tenant_id}` | Verified member | Tenant details. |
| `GET /tenants/{tenant_id}/members` | Verified member | List the members. |
| `GET /tenants/{tenant_id}/invites` | Verified member | List the open invites. |
| `POST /tenants/{tenant_id}/invites` | Verified member | Create an invite, or send it again. |
| `DELETE /tenants/{tenant_id}/invites/{invite_id}` | Verified member | Cancel an invite. |

A user who is not a member of a tenant gets `404` on the routes of that tenant.

The API runs behind a proxy. It reads the client IP for the rate limits from the last entry of `X-Forwarded-For`. Do not open the API port to the internet.

## Environment variables

The API reads these in `src/config.rs`. It also loads a `.env` file if one exists. See `.env.example`.

| Name | Default | Use |
| --- | --- | --- |
| `DATABASE_URL` | none (required) | Postgres connection string |
| `BIND_ADDR` | `0.0.0.0:3000` | Address the API binds to |
| `DB_MAX_CONNECTIONS` | `10` | Maximum size of the database connection pool |
| `CORS_ALLOWED_ORIGINS` | `https://app.dearest.rsvp` | Browser origins that may call the API. Separate origins with a comma |
| `APP_BASE_URL` | `https://app.dearest.rsvp` | Base URL for links in email |
| `SMTP_HOST` | none (required) | SMTP server |
| `SMTP_PORT` | `465` | SMTP port |
| `SMTP_USERNAME` | none | SMTP user |
| `SMTP_PASSWORD` | none | SMTP password. Keep it secret |
| `SMTP_TLS` | `true` | Use TLS. Set to `false` for Mailpit |
| `EMAIL_FROM` | `Dearest <hello@dearest.rsvp>` | Sender of email |
| `COOKIE_SECURE` | `true` | Set the `Secure` attribute on the session cookie |
| `RUST_LOG` | `info,tower_http=debug` | Log filter |
| `API_PORT` | `3000` | Host port that Docker Compose publishes |

## Run the API on the host

You need a Rust toolchain and a macOS or Linux C toolchain. Start only the database:

```bash
docker compose up -d db
export DATABASE_URL=postgres://postgres:postgres@localhost:5432/dearest
cargo run
```

The API applies pending migrations when it starts.

## Migrations

There are no tables yet. The migration list is in `migration/src/lib.rs`.

Install the SeaORM CLI one time:

```bash
cargo install sea-orm-cli
```

Create a migration:

```bash
sea-orm-cli migrate generate create_example_table
```

Add the new migration to `Migrator::migrations()` in `migration/src/lib.rs`. Then run the migrations:

```bash
sea-orm-cli migrate up
```

`sea-orm-cli` reads `DATABASE_URL` from the environment.

## Tests

Some tests use a mock database. The integration tests need Postgres. Start it first:

```bash
docker compose up -d db
cargo test --workspace
```

Each integration test makes its own database on the server in `TEST_DATABASE_URL`. The default is `postgres://postgres:postgres@localhost:5432/postgres`. Run `docker compose down -v` to remove the test databases.

## OpenAPI spec

The server sends the API spec at `GET /openapi.json`. The code builds the spec in `src/openapi.rs`. No file in this repository holds a copy.

Other repositories make their API client from the deployed spec at `https://api.dearest.rsvp/openapi.json`. They do not keep a copy. A change to the API reaches a client after the API deploys.

```mermaid
flowchart LR
    code["Rust code and utoipa annotations"] --> route["GET /openapi.json"]
    route --> live["api.dearest.rsvp"]
    live --> web["dearest.rsvp-web: yarn install makes the client"]
    live --> other["Future repository: makes its own client"]
```

Run the API on your computer and read `http://localhost:3000/openapi.json` to see a change before it deploys.

Two checks keep the spec usable:

- The tests in `tests/openapi_route.rs` and `src/openapi.rs` run in CI on each pull request.
- The `publish` job calls the deployed `/openapi.json` after each deploy and fails if the spec is not usable.

## Git hooks

The repository has Git hooks in `.githooks/`. Turn them on one time in each clone:

```bash
git config core.hooksPath .githooks
```

- `pre-commit` runs `cargo fmt --all --check`.
- `pre-push` runs `cargo clippy --workspace --all-targets -- -D warnings`.

These are the same checks that CI runs on a pull request. To skip a hook one time, add `--no-verify` to the Git command.

## Layout

- `src/config.rs`: configuration struct, read from the environment
- `src/lib.rs`: router, health route, and state
- `src/main.rs`: start-up, database connection, server, and shutdown
- `src/error.rs`: `AppError` and the error codes
- `src/auth/`: passwords, tokens, sessions, extractors, and the `/auth` routes
- `src/email/`: `EmailSender` trait, SMTP sender, test sender, and templates
- `src/tenants.rs`: `/tenants` routes
- `src/invites.rs`: `/invites` routes and the invite routes of a tenant
- `src/rate_limit.rs`: client IP extractor and rate limits
- `src/cleanup.rs`: the hourly task that deletes old rows
- `src/entities/`: SeaORM entities, generated from the database
- `migration/`: migration crate and its CLI binary
- `tests/`: integration tests
- `.githooks/`: Git hooks for format and lint checks
- `Dockerfile`: multi-stage build with cargo-chef dependency caching
