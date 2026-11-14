# Auth and tenants: design

Date: 2026-11-14
Repositories: `dearest.rsvp-api` (most of the work) and `dearest.rsvp-web`.

## Intent

Dearest is a wedding website and RSVP app. A tenant is one wedding. The people who plan a wedding (the couple, and sometimes a wedding planner) need accounts so that they can manage the wedding together.

This work adds:

- accounts with an email and a password
- email verification
- password reset
- tenants with any number of members
- invites to a tenant by email
- an email transport layer

## Goals

- Each person has one account. The account identifies the person with an email and a password.
- The API stores only a hash of each password (argon2id).
- A user must verify the email address before the user can do anything else in the app.
- A tenant has 1 or more members. A user can be a member of 0 or more tenants.
- A member can invite a person to the tenant by email.
- A user who is a member of 2 or more tenants can switch between them without a new login.
- A user who is a member of 0 or 1 tenants never sees a tenant list, a switcher, or any sign that other tenants exist.

## Non-goals

These are out of scope for this work:

- roles or permissions inside a tenant (all members have the same access)
- account deletion
- leaving a tenant, or removing a member
- a change of email address
- two-factor authentication
- login with a third-party identity provider
- the wedding website, guests, and RSVPs
- the help page that links to `/new-wedding`

## Anti-goals

The work fails, even if it runs, if any of these is true:

- A user can see the name or the ID of a tenant that the user is not a member of. One exception: a person with a valid invite token sees the name of the tenant in that invite (the email and `GET /invites/:token`). The invite exists to show this name. (Builder call.)
- A forwarded invite link lets a person with a different email join the tenant.
- An unverified user can read or change tenant data.
- A database leak gives an attacker a usable session, invite, verification, or reset token.

## Constraints

- The API is Rust: axum 0.8, SeaORM 2, Postgres 18. It runs migrations at start-up.
- The web app is React 19, Mantine, TanStack Query, Vite+. nginx serves it as a single-page app.
- Both run on one VPS behind the Dokploy proxy. The API is `https://api.dearest.rsvp`. The web app is `https://app.dearest.rsvp`.
- The two hosts are the same site (`dearest.rsvp`). A cookie with `SameSite=Lax` on the API host goes with `fetch` calls from the web app.
- In local development, Vite forwards `/api` to the API. The browser sees one origin.
- The email provider is Resend.

## Approaches

The main choice is how the app knows the current tenant. All approaches use a server-side session in a cookie.

1. **Tenant ID in the URL (chosen).** Web pages are under `/w/:tenantId/...`. Tenant API routes are under `/tenants/:tenantId/...`. The API checks the membership on each request. A switch is a page navigation. Two browser tabs can show two different tenants. A link opens the correct tenant.
2. **Current tenant on the session row.** A switch calls the API to change `current_tenant_id`. All tabs share one current tenant. A switch in one tab makes another tab write to the wrong tenant. We rejected this approach because of that data risk.
3. **JWT instead of a server-side session.** The API cannot cancel a JWT before it expires. Logout and password reset then cannot stop other sessions. It also needs refresh tokens. We rejected this approach. One API on one VPS does not need it.

## Decisions

### Data model

All IDs are UUIDs. All times are `timestamptz`.

`users`

- `id`
- `email`: stored in lowercase, trimmed. Unique.
- `name`: display name, for example in invite emails.
- `password_hash`: argon2id PHC string.
- `email_verified_at`: null until the user verifies.
- `created_at`, `updated_at`

`tenants`

- `id`
- `name`: for example "Alex & Sam".
- `created_at`, `updated_at`

`user_tenant_memberships`

- `user_id`, `tenant_id`: primary key on the pair. Both are foreign keys.
- `created_at`

`sessions`

- `id`
- `user_id`
- `token_hash`: SHA-256 of the session token. Unique.
- `created_at`, `last_seen_at`, `expires_at`

`email_tokens` (email verification and password reset)

- `id`
- `user_id`
- `purpose`: `verify_email` or `reset_password`
- `token_hash`: SHA-256 of the token. Unique.
- `expires_at`, `used_at`, `created_at`

`invites`

- `id`
- `tenant_id`
- `email`: lowercase, trimmed
- `invited_by`: user ID
- `token_hash`: SHA-256 of the token. Unique.
- `expires_at`, `accepted_at`, `cancelled_at`, `created_at`
- A partial unique index on `(tenant_id, email)` where `accepted_at` and `cancelled_at` are null. A tenant has at most one open invite for each email.

### Tokens

- Session, verification, reset, and invite tokens are 32 random bytes from the OS random source, encoded as base64url.
- The database stores only the SHA-256 hash of a token. A fast hash is safe here because the tokens have high entropy.
- Each token works one time, except the session token.

| Token | Lifetime |
| --- | --- |
| Session | 30 days. Each request moves `expires_at` forward to 30 days from now. To limit writes, update the row at most one time each hour. |
| Email verification | 24 hours |
| Password reset | 1 hour |
| Invite | 7 days |

### Passwords

- Hash with argon2id. Use the OWASP parameters: 19 MiB memory, 2 iterations, 1 lane.
- Run the hash and the check in `tokio::task::spawn_blocking`.
- There are no password rules. The API rejects only an empty password.
- The axum request body limit (2 MB by default) sets the maximum length. argon2 reduces a long input with a fast hash first, so a long password does not make the hash slow.
- Login with an unknown email still runs one hash check against a fixed dummy hash, so response time does not show if an account exists.

### Session cookie

- Name: `dearest_session`.
- Attributes: `HttpOnly`, `Secure`, `SameSite=Lax`, `Path=/`, no `Domain` (host-only on the API host), `Max-Age` 30 days.
- The `COOKIE_SECURE` setting can turn off `Secure` for local HTTP if a browser needs it.

### CSRF protection

- The `SameSite=Lax` cookie does not go with a cross-site write request.
- The API also checks the `Origin` header on each `POST`, `PUT`, `PATCH`, and `DELETE`. It rejects the request with `403` if the origin is not in `CORS_ALLOWED_ORIGINS`.
- CORS sets `allow_credentials(true)`. The allowed origins stay an explicit list.

### Verification gate

- The auth middleware loads the session and the user.
- If the user is not verified, the API answers `403` with `{"error": "email_not_verified"}` on all routes except the ones in the "Unverified" column below.
- The web app shows only the "check your inbox" screen to an unverified user. That screen has a resend button and a logout button.

### Tenant access

- Each `/tenants/:tenantId/...` route checks that the user has a membership row for that tenant.
- If there is no membership, the API answers `404`, not `403`. The answer is the same as for a tenant that does not exist.
- `GET /auth/me` returns only the user's own memberships.

### Invites

- Any verified member can invite an email to the tenant.
- If the email already belongs to a member, the API answers `409`.
- A second invite to the same email replaces the token of the open invite and sets a new expiry. This is the "send again" action.
- A member can cancel an open invite.
- The invite binds to the invited email:
  - If no account has that email, the invitee sets a name and a password. The API creates the user as verified, adds the membership, and starts a session.
  - If an account has that email, the invitee must log in to that account first. Then the API adds the membership.
  - If a different account is logged in, the API answers `403` with `{"error": "invite_email_mismatch"}`. The web app shows "This invite is for x@...".
- Acceptance of an invite marks the user as verified. The link went to that inbox, so this proves the user owns the address.

### Password reset

- The request always answers `202`. The API sends the email only if the account exists.
- A successful reset sets the new password, marks the token as used, and deletes all sessions of the user. The user then logs in again.
- A reset also marks the email as verified, because the link went to that inbox.

### Signup

- Signup creates the user (unverified), creates a session, sets the cookie, and sends the verification email.
- If the email already has an account, the API answers `409`. This shows that an account exists. We accept this risk for a wedding app. Rate limits slow down enumeration. (Builder call.)
- Signup does not create a tenant. After verification, the web app sends a user with 0 memberships to onboarding. Onboarding creates the first tenant.
- Any verified user can create more tenants with `POST /tenants`. The web app has a `/new-wedding` page for this, but no link in the app points to it. A help page (out of scope for this work) will link to it and explain it. `/onboarding` and `/new-wedding` share one form component.

### Rate limits

Use `tower_governor` with in-memory state. There is one API instance, so in-memory state is enough. Use these limits as a start (builder call):

| Route | Limit |
| --- | --- |
| Login | 10 per minute for each IP, 5 per minute for each email |
| Signup | 5 per hour for each IP |
| Password reset request | 5 per hour for each IP, 3 per hour for each email |
| Verification resend | 3 per hour for each user |
| Invite create or resend | 20 per hour for each user |

- A blocked request gets `429`.
- The API is behind the Dokploy proxy (Traefik). Read the client IP from the last entry of `X-Forwarded-For`. The proxy adds that entry, so a client cannot fake it. Do not use governor `SmartIpKeyExtractor`, because it reads the first entry, and a client can set the first entry. Write a small custom key extractor. (Builder call.)
- Do not expose the API port to the internet directly. If you do, a client can fake the header.

### Email transport

- Add an `EmailSender` trait in the API with one method that sends a message (to, subject, text body, HTML body).
- Production implementation: SMTP with `lettre`, to `smtp.resend.com` on port 465 (TLS). The username is `resend`. The password is the Resend API key.
- Local development: Mailpit in `docker-compose.yml`, with SMTP on port 1025 and the web UI on port 8025. The same SMTP implementation works with it.
- Tests: an in-memory implementation that records the messages.
- Send email after the database transaction commits:
  1. The handler opens a transaction with `db.begin()`, writes the rows (for example the user and the token hash), and calls `txn.commit()`.
  2. After the commit returns `Ok`, the handler builds the message and starts the send with `tracker.spawn(...)`. `tracker` is a `tokio_util::task::TaskTracker` in `AppState`.
  3. The handler returns its response. It does not wait for the send.
  4. The spawned task calls `EmailSender::send`. If the send fails, it logs the error with `tracing::error!`. The request does not fail. The user can use the resend action.
- This order means that the API never sends a link for a token that is not in the database.
- On shutdown, `main` calls `tracker.close()` and `tracker.wait()` after the server stops, with a 10-second limit. Thus a deploy does not drop an email that is in progress.
- Known limit: if the process crashes between the commit and the send, the email does not go. The user must press resend. An outbox table with a retry worker removes this limit, but it is more work. We do not need it at this scale.
- Each email has a plain-text body and a simple HTML body.
- Links in email use `APP_BASE_URL`.
- The `From` domain needs SPF, DKIM, and DMARC DNS records. Resend shows the records when you add the domain.

### Cleanup

A task inside the API process deletes old rows each hour:

1. At start-up, `main` starts the task with `tokio::spawn`, after the migrations.
2. The task uses `tokio::time::interval` with a period of 1 hour. The first tick runs at start-up.
3. On each tick, it runs three `DELETE` statements:
   - sessions where `expires_at < now()`
   - email tokens where `expires_at < now()` or `used_at` is not null
   - invites where `expires_at < now() - interval '30 days'`
4. It logs the number of deleted rows. If a statement fails, it logs the error and waits for the next tick.
5. A `tokio_util::sync::CancellationToken` stops the loop when the shutdown signal arrives.

There is one API instance, so only one task runs. During a deploy, the old and the new containers can both run the task for a short time. That is safe, because a `DELETE` of expired rows gives the same result if it runs two times.

The API does not need these deletes to be correct. Each token check also compares `expires_at` with the current time. The task only keeps the tables small.

### API endpoints

All bodies are JSON. Errors use `{"error": "<code>"}`.

| Method and path | Auth | Unverified | Purpose |
| --- | --- | --- | --- |
| `POST /auth/signup` | none | yes | `{email, password, name}`. Creates user and session. Sends verification email. |
| `POST /auth/login` | none | yes | `{email, password}`. `401 invalid_credentials` for a wrong email or password. |
| `POST /auth/logout` | session | yes | Deletes the session. Clears the cookie. |
| `GET /auth/me` | session | yes | `{user: {id, email, name, email_verified}, memberships: [{tenant_id, tenant_name}]}` |
| `POST /auth/verify-email` | none | yes | `{token}`. Marks the user as verified. |
| `POST /auth/verify-email/resend` | session | yes | Sends a new verification email. Older tokens stop working. |
| `POST /auth/password-reset` | none | yes | `{email}`. Always `202`. |
| `POST /auth/password-reset/confirm` | none | yes | `{token, password}` |
| `GET /invites/:token` | none | yes | `{tenant_id, tenant_name, inviter_name, email, account_exists}`. `404` if the token is not valid. The web app uses `tenant_id` to open the tenant after `409 already_member`. |
| `POST /invites/:token/accept` | optional | yes | No session: `{name, password}` creates the account. Session: adds the membership. |
| `POST /tenants` | session | no | `{name}`. Creates the tenant and a membership for the user. |
| `GET /tenants/:tenantId` | member | no | Tenant details. |
| `GET /tenants/:tenantId/members` | member | no | `[{user_id, name, email}]` |
| `GET /tenants/:tenantId/invites` | member | no | Open invites. |
| `POST /tenants/:tenantId/invites` | member | no | `{email}`. Creates an invite or sends it again. |
| `DELETE /tenants/:tenantId/invites/:inviteId` | member | no | Cancels the invite. |

### New configuration

| Name | Example | Use |
| --- | --- | --- |
| `APP_BASE_URL` | `https://app.dearest.rsvp` | Base for links in email |
| `SMTP_HOST` | `smtp.resend.com` | SMTP server |
| `SMTP_PORT` | `465` | SMTP port |
| `SMTP_USERNAME` | `resend` | SMTP user |
| `SMTP_PASSWORD` | Resend API key | SMTP password. Keep it secret. |
| `SMTP_TLS` | `true` | Use TLS. Set to `false` for Mailpit. |
| `EMAIL_FROM` | `Dearest <hello@dearest.rsvp>` | Sender |
| `COOKIE_SECURE` | `true` | `Secure` attribute on the session cookie |

### Web app

- Add TanStack Router. It gives typed routes and works with TanStack Query.
- `apiFetch` sends `credentials: "include"`.
- A `useMe` query loads `GET /auth/me`. The route guards use it.

Pages:

| Path | Who | Content |
| --- | --- | --- |
| `/login` | logged out | Email and password. Links to signup and forgot password. |
| `/signup` | logged out | Name, email, password. |
| `/verify-email` | unverified | "Check your inbox". Resend button. Logout button. |
| `/verify-email/confirm?token=` | anyone | Calls verify. Then goes to `/`. |
| `/forgot-password` | logged out | Email. |
| `/reset-password?token=` | anyone | New password. Then goes to `/login`. |
| `/invite/:token` | anyone | Shows the tenant and the inviter. Lets the user set a password, log in, or accept. |
| `/onboarding` | verified, 0 tenants | Wedding name. Creates the tenant. |
| `/new-wedding` | verified | Wedding name. Creates another tenant. Then goes to `/w/:tenantId`. No link in the app points to this page. A future help page will link to it. |
| `/w/:tenantId` | member | Wedding home (placeholder for now). |
| `/w/:tenantId/members` | member | Members, open invites, invite form. |

Routing rules for `/`:

- Not logged in: go to `/login`.
- Not verified: go to `/verify-email`.
- 0 memberships: go to `/onboarding`.
- 1 membership: go to `/w/:tenantId`.
- 2 or more memberships: go to `/w/:tenantId` for the first membership, or the last tenant the user opened. The web app stores that in `localStorage`.

Tenant switcher:

- Show it in the header only when the user has 2 or more memberships.
- It lists only the user's memberships, from `GET /auth/me`.
- A choice navigates to `/w/:tenantId`.

### Tests

- Unit tests for token creation and hashing, password rules, config, and the `Origin` check.
- Integration tests for each endpoint against a real Postgres. Add a Postgres 18 service to the API CI job. The current mock-database tests stay.
- Web unit tests for the redirect rules and the switcher visibility.

## Left to the builder

- Module layout in the API.
- Exact error codes, other than the ones in this document.
- Email copy and HTML layout.
- Mantine layout of each page.
- The order of the work and how to split it into pull requests.
