# OpenAPI types for the web frontend: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The backend makes an OpenAPI spec from its Rust code. The frontend generates its types, client, and query options from that spec.

**Architecture:** Named response structs replace each `json!` response. `utoipa` and `utoipa-axum` build the spec from these structs and from annotated handlers. A test writes the spec to `openapi.json` and fails if the file is stale. The frontend commits a copy of that file and generates code with `@hey-api/openapi-ts`. The hand-written hooks keep their cache logic and call the generated SDK.

**Tech Stack:** Rust 2024, axum 0.8, sea-orm 2, `utoipa`, `utoipa-axum`. React 19, TanStack Query 5, TypeScript 6, yarn 4, Vite+ (`vp`), `@hey-api/openapi-ts`.

**Spec:** `docs/superpowers/specs/2026-11-14-openapi-types-design.md` (in `dearest.rsvp-api`). Read it before you start.

Repositories (siblings in `/Users/willgardner/Documents/GitHub/`): backend `dearest.rsvp-api`, frontend `dearest.rsvp-web`. Tasks 1 to 4 run in the backend. Tasks 5 to 8 run in the frontend. Merge the backend branch before Task 5.

## Global Constraints

- The JSON of every response stays the same as today: same names, same `null` rules, same uuid and date formats. The API URLs, methods, and status codes do not change.
- OpenAPI 3.1. The spec file is `openapi.json` in the backend root.
- The session is an HttpOnly cookie. The generated client sends `credentials: "include"`.
- The generated client throws `ApiError(status, body)` for a non-2xx response. An empty body is `null`. `fetchMe` returns `null` on 401.
- The `error` field of an error body is a value of one `ErrorCode` enum. `AppError::parts` uses the same enum.
- A route lists only the error codes it can return.
- Do not edit generated code by hand. Pin the exact `@hey-api/openapi-ts` version. Commit the generated code in `src/api/generated/`.
- The spec writer needs no database.
- Both repositories start from an up to date `main`. Make a new branch `openapi-types` in each.
- Commit messages and code comments use ASD-STE100 style. End each commit with the attribution line from the session.

## Review Focus

Inputs and conditions that no spec section tests directly. The owning task adds the test.

1. `GET /tenants/{id}/invites` where the inviter user is gone: `invited_by_name` is `null`. The hand-written type says `string`. The generated type must be `string | null`. (Task 2, Task 6)
2. A 204 or 202 response with no body: the client returns `null`, not a parse error. (Task 6)
3. `POST /invites/{token}/accept` with a session cookie and an empty body, and with a name and password body and no cookie. The handler reads raw `Bytes`, so the spec must describe an optional body. (Task 3)
4. `GET /health` while the database is down: status 503 with `{ "status": "degraded", "db": "down" }`. (Task 2, Task 3)
5. An invite token or tenant id with a character that needs URL encoding: the generated client encodes the path parameter. (Task 6)

---

### Task 1: Backend branch and response-shape tests

**Files:**
- Create: `tests/response_shapes.rs`
- Read: `tests/harness.rs`, `tests/tenants.rs` (copy their request pattern)

**Interfaces:**
- Produces: a test file that asserts the full JSON body of each route that returns JSON. Tasks 2 and 3 rely on it as proof that the JSON did not change.

- [ ] **Step 1: Make the branch.** In `dearest.rsvp-api`: `git switch main && git pull && git switch -c openapi-types`.
- [ ] **Step 2: Run the baseline.** Run `cargo test`. Expected: all tests pass. If the database is not up, start it as the README says.
- [ ] **Step 3: Write the tests in `tests/response_shapes.rs`.** One test for each of these routes. Each test asserts the whole body with `assert_eq!(response.body, json!({...}))`. Use `assert_eq!` on ids and dates only through a helper that checks the string format (uuid v7 and RFC 3339), and compare the other fields exactly:
  - `signup_body_has_me_shape`: `POST /auth/signup`, status 201, keys `user` (`id`, `email`, `name`, `email_verified`) and `memberships`.
  - `login_body_has_me_shape`, `me_body_has_me_shape` (`GET /auth/me`).
  - `create_tenant_body`, `get_tenant_body`, `list_members_body` (keys `user_id`, `name`, `email`).
  - `create_invite_body` (keys `id`, `email`, `expires_at`).
  - `list_invites_body` (keys `id`, `email`, `invited_by_name`, `expires_at`) and `list_invites_inviter_deleted_gives_null` (Review Focus 1).
  - `invite_preview_body` (keys `tenant_id`, `tenant_name`, `inviter_name`, `email`, `account_exists`).
  - `accept_invite_body` (key `tenant_id`), for the signed-in case and the new-account case.
  - `health_ok_body` and, if the harness can stop the database, `health_down_body` (Review Focus 4). Skip the second one if the harness cannot do it, and say so in the commit message.
  A test that exists already and asserts the full body may stay as it is. Do not duplicate it.
- [ ] **Step 4: Run the tests.** Run `cargo test --test response_shapes`. Expected: PASS. The tests describe today's behavior, so they pass with no code change.
- [ ] **Step 5: Commit.** `git add tests/response_shapes.rs && git commit -m "test: assert the full body of each JSON response"`.

---

### Task 2: Response structs and the `ErrorCode` enum

**Files:**
- Create: `src/dto.rs`
- Modify: `Cargo.toml`, `src/lib.rs` (add `pub mod dto;`), `src/error.rs`, `src/auth/handlers.rs`, `src/tenants.rs`, `src/invites.rs`, `src/lib.rs` (`health`)

**Interfaces:**
- Consumes: the tests of Task 1.
- Produces, in `src/dto.rs`, all with `#[derive(Serialize, utoipa::ToSchema)]`:
  - `Me { user: MeUser, memberships: Vec<MembershipSummary> }`
  - `MeUser { id: Uuid, email: String, name: String, email_verified: bool }`
  - `MembershipSummary { tenant_id: Uuid, tenant_name: String }`
  - `Tenant { id: Uuid, name: String }`
  - `Member { user_id: Uuid, name: String, email: String }`
  - `CreatedInvite { id: Uuid, email: String, expires_at: DateTime<FixedOffset> }`
  - `Invite { id: Uuid, email: String, invited_by_name: Option<String>, expires_at: DateTime<FixedOffset> }`
  - `InvitePreview { tenant_id: Uuid, tenant_name: String, inviter_name: String, email: String, account_exists: bool }`
  - `AcceptedInvite { tenant_id: Uuid }`
  - `Health { status: String, db: String }`
  - Use the date type that the sea-orm entity field has, so the JSON stays the same.
- Produces, in `src/error.rs`: `pub enum ErrorCode` (derive `Serialize`, `ToSchema`, `Clone, Copy`; `#[serde(rename_all = "snake_case")]`) with one variant for each code of `AppError::parts`; `pub struct ErrorBody { pub error: ErrorCode }` (derive `Serialize`, `ToSchema`); `AppError::parts` returns `(StatusCode, ErrorCode)`.

- [ ] **Step 1: Add the dependencies.** In `Cargo.toml` add `utoipa` and `utoipa-axum` at their latest versions, with the `uuid` and `chrono` features of `utoipa`. Run `cargo build`. Expected: builds.
- [ ] **Step 2: Add a failing test for `ErrorCode`.** In the test module of `src/error.rs`, add `error_code_serializes_to_the_wire_string`: for each `AppError` variant, `serde_json::to_value(code)` equals the string the old `parts` returned (for example `"email_not_verified"`). Run `cargo test error_code`. Expected: FAIL, `ErrorCode` is not defined.
- [ ] **Step 3: Add `ErrorCode` and `ErrorBody`.** Change `parts` and `into_response` to use them. The response body is `Json(ErrorBody { error: code })`. Run the test. Expected: PASS.
- [ ] **Step 4: Add the structs in `src/dto.rs`.** Replace `me_body` (return type `AppResult<Me>`), and each `json!` response in `tenants.rs`, `invites.rs`, `lib.rs` with the structs. Change each handler return type from `Json<Value>` to `Json<T>`.
- [ ] **Step 5: Run all tests.** Run `cargo test`. Expected: PASS, including `tests/response_shapes.rs`. A failure means the JSON changed: fix the struct, not the test.
- [ ] **Step 6: Commit.** `git commit -am "refactor: return named structs and an ErrorCode enum"` (add `src/dto.rs` first).

---

### Task 3: `utoipa` annotations and the OpenAPI document

**Files:**
- Create: `src/openapi.rs`
- Modify: `src/lib.rs`, `src/auth/mod.rs`, `src/auth/handlers.rs`, `src/auth/verify.rs`, `src/auth/reset.rs`, `src/tenants.rs`, `src/invites.rs`, request body structs (add `ToSchema`)

**Interfaces:**
- Consumes: the structs and `ErrorBody` of Task 2.
- Produces, in `src/openapi.rs`: `pub fn spec() -> utoipa::openapi::OpenApi`. It builds the router without `AppState` data and returns its spec. Each route module exposes `pub fn router() -> OpenApiRouter<AppState>` (replace `Router<AppState>`).

- [ ] **Step 1: Write a failing test in `src/openapi.rs`.** `spec_lists_every_route`: collect `(method, path)` pairs from `spec()` and assert they equal this set exactly: `GET /health`; `POST /auth/signup`, `/auth/login`, `/auth/logout`, `/auth/verify-email`, `/auth/verify-email/resend`, `/auth/password-reset`, `/auth/password-reset/confirm`; `GET /auth/me`; `POST /tenants`; `GET /tenants/{tenant_id}`, `/tenants/{tenant_id}/members`, `/tenants/{tenant_id}/invites`; `POST /tenants/{tenant_id}/invites`; `DELETE /tenants/{tenant_id}/invites/{invite_id}`; `GET /invites/{token}`; `POST /invites/{token}/accept`. Run it. Expected: FAIL.
- [ ] **Step 2: Annotate each handler with `#[utoipa::path]`.** Give each a `tag`, the success status and body type, and `responses` for the errors it can return, with body `ErrorBody`. Read each handler and its extractors to find the codes. Examples: `AnyUser` adds 401 `unauthenticated`; `VerifiedUser` also adds 403 `email_not_verified`; every write route adds 403 `forbidden_origin`; a rate-limited route adds 429 `rate_limited`; every route with `ApiJson` adds 400 `invalid_input`. `/health` lists 200 and 503, both with `Health`.
- [ ] **Step 3: Describe the special bodies.** `ApiJson<T>` needs `request_body = T`. `accept` reads raw `Bytes`: declare `request_body(content = NewAccountBody, required = false)` (Review Focus 3).
- [ ] **Step 4: Declare the session cookie.** Add an API key security scheme of type cookie in `ApiDoc`. Name it as the session cookie in `src/auth/session.rs`. Mark each session route with `security(("session" = []))`. Public routes get none.
- [ ] **Step 5: Switch the routers to `OpenApiRouter` and `routes!`.** Compose them in `src/openapi.rs` with the same `nest` paths as `app()` in `src/lib.rs`. `app()` takes the router from the same function, so the served routes and the spec cannot differ. Keep the layers in `app()` as they are.
- [ ] **Step 6: Run the tests.** Run `cargo test`. Expected: PASS, with the route test and all shape tests.
- [ ] **Step 7: Commit.** `git commit -am "feat: describe the API with utoipa"`.

---

### Task 4: Write `openapi.json`, check it, run it in CI

**Files:**
- Create: `openapi.json` (generated), `src/bin/openapi.rs`
- Modify: `tests/` (add `tests/openapi_spec.rs`), `.github/workflows/ci.yml`, `README.md`

**Interfaces:**
- Consumes: `dearest::openapi::spec()`.
- Produces: `cargo run --bin openapi` prints the spec as pretty JSON with a trailing newline. `openapi.json` holds that output. The frontend copies this file.

- [ ] **Step 1: Write a failing test `committed_spec_is_current` in `tests/openapi_spec.rs`.** It reads `openapi.json`, parses it, and compares it to `serde_json::to_value(spec())`. On a difference the message says: "Run `cargo run --bin openapi > openapi.json`." Run it. Expected: FAIL, the file does not exist.
- [ ] **Step 2: Add `src/bin/openapi.rs`.** It prints `spec().to_pretty_json()` and needs no environment variable and no database.
- [ ] **Step 3: Generate the file.** Run `cargo run --bin openapi > openapi.json`. Open it and check: the error enum lists all codes, `Invite.invited_by_name` is nullable, the session security scheme exists.
- [ ] **Step 4: Run the test.** Run `cargo test --test openapi_spec`. Expected: PASS.
- [ ] **Step 5: CI.** The existing `cargo test` step already runs the new test. Confirm this in `ci.yml`. Add nothing if it does.
- [ ] **Step 6: Document.** Add a short section to `README.md`: how to regenerate `openapi.json`, and that the frontend copies it.
- [ ] **Step 7: Commit and merge.** Commit with `feat: write and check openapi.json`. Open a pull request. Merge it to `main` before Task 5.

---

### Task 5: Frontend generator setup

**Files:**
- Create: `openapi-ts.config.ts`, `openapi.json` (copy), `scripts/sync-openapi.mjs`, `src/api/generated/` (generated), `src/api/clientConfig.ts`
- Modify: `package.json`, `src/api/client.ts` (keep `ApiError`), `src/api/index.ts`

**Interfaces:**
- Consumes: the merged `openapi.json` of the backend.
- Produces: scripts `api:sync` (copy `../dearest.rsvp-api/openapi.json` to `./openapi.json`), `api:generate` (run the generator), and `api:check` (run the generator, then `git diff --exit-code src/api/generated openapi.json`). `src/api/clientConfig.ts` exports `configureApiClient(): void`, called once in the app entry file before the first request.

- [ ] **Step 1: Make the branch.** In `dearest.rsvp-web`: `git switch main && git pull && git switch -c openapi-types`.
- [ ] **Step 2: Install.** `yarn add -D -E @hey-api/openapi-ts`. The `-E` flag pins the exact version. Run `yarn api:generate` once the config exists.
- [ ] **Step 3: Write `openapi-ts.config.ts`.** Input `./openapi.json`, output `src/api/generated`, plugins: `@hey-api/client-fetch`, `@hey-api/sdk`, `@hey-api/typescript`, `@tanstack/react-query`. Set the SDK to throw on error (`throwOnError: true`).
- [ ] **Step 4: Write a failing test `src/api/clientConfig.test.ts`.** Mock global `fetch`. After `configureApiClient()`: a call to a generated function sends `credentials: "include"`; a 500 response with body `{"error":"internal"}` rejects with an `ApiError` of status 500 and that body; a 204 response resolves to `null` (Review Focus 2); the URL starts with `import.meta.env.VITE_API_BASE_URL ?? ""`. Run `yarn test`. Expected: FAIL.
- [ ] **Step 5: Implement `configureApiClient`.** Use the client `setConfig` call for `baseUrl` and `credentials`. Add an error interceptor or a custom `fetch` that throws `ApiError` as `apiFetch` does. Run the test. Expected: PASS.
- [ ] **Step 6: Add the scripts and generate.** Run `yarn api:sync && yarn api:generate`. Run `yarn typecheck`. Expected: no errors.
- [ ] **Step 7: Commit.** `git add -A && git commit -m "feat: add the OpenAPI generator and the generated client"`.

---

### Task 6: Move the hooks to the generated client

**Files:**
- Modify: `src/api/auth.ts`, `src/api/invites.ts`, `src/api/tenants.ts`, `src/api/health.ts`, `src/api/index.ts`, `src/api/auth.test.ts`, `src/api/client.test.ts`, the pages that import from `./types`
- Delete: `src/api/types.ts`

**Interfaces:**
- Consumes: the generated SDK functions (for example `login`, `signup`, `getMe`) and types from `src/api/generated/`. Read the generated names before you write code.
- Produces: the same exported names as today (`useMe`, `useLogin`, `meKeys`, `meQueryOptions`, `fetchMe`, `refreshMe`, `clearSession`, `inviteKeys`, `tenantKeys`, and the rest). Each hook calls the generated SDK. `types` are re-exported from the generated types under the old names (`Me`, `Tenant`, `Member`, `Invite`, `InvitePreview`).

- [ ] **Step 1: Run the existing tests.** `yarn test`. Expected: PASS, a green baseline.
- [ ] **Step 2: Add failing tests.** In `src/api/invites.test.ts` (new): `list_invites_accepts_null_inviter` (a response with `invited_by_name: null` type-checks and the hook returns it); `invite_path_parameters_are_encoded` (a token `a/b c` appears as `a%2Fb%20c` in the requested URL, Review Focus 5). In `auth.test.ts` keep the 401 to `null` test for `fetchMe`. Run. Expected: FAIL.
- [ ] **Step 3: Change `auth.ts`.** Replace each `apiFetch` call with the generated function. Keep `meKeys`, `fetchMe` (401 to `null`), `meQueryOptions`, `refreshMe`, `refreshMeAfterSuccess`, and `clearSession` unchanged in behavior. Use the generated query options only for a hook with no custom cache logic.
- [ ] **Step 4: Change `invites.ts`, `tenants.ts`, `health.ts`.** Same rule. Keep `inviteKeys` and `tenantKeys`. Remove the hand-made `invitesPath` helper if nothing uses it.
- [ ] **Step 5: Delete `src/api/types.ts`.** Fix imports. If `HealthResponse` was exported, re-export the generated type under that name.
- [ ] **Step 6: Run checks.** `yarn typecheck && yarn test && yarn check`. Expected: all pass. Fix the pages with small changes only. A page change that is more than an import or a type name needs a note in the commit message.
- [ ] **Step 7: Commit.** `git add -A && git commit -m "refactor: call the API through the generated client"`.

---

### Task 7: Type the error messages over `ErrorCode`

**Files:**
- Modify: `src/api/errors.ts`, `src/api/errors.test.ts`

**Interfaces:**
- Consumes: the generated `ErrorCode` type (name may differ: use the generated name for the `error` enum).
- Produces: `errorCode(error: unknown): ErrorCode | null` and `errorMessage(error: unknown): string`, with the same behavior as today.

- [ ] **Step 1: Write the failing test.** In `errors.test.ts` add `messages_cover_known_codes_and_fall_back`: for `invalid_credentials` the message is the existing copy; for an unknown string code and for a body with no `error` field, `errorMessage` returns the fallback text. Add a compile-time check: `const MESSAGES: Partial<Record<ErrorCode, string>>`, so a typo in a key is a type error. Run `yarn typecheck`. Expected: FAIL until the types are used.
- [ ] **Step 2: Change `errors.ts`.** Type `MESSAGES` as above. `errorCode` returns the code only when it is a known value.
- [ ] **Step 3: Run checks.** `yarn typecheck && yarn test`. Expected: PASS.
- [ ] **Step 4: Commit.** `git commit -am "refactor: type the error messages over the ErrorCode enum"`.

---

### Task 8: Frontend CI check and docs

**Files:**
- Modify: `.github/workflows/ci.yml`, `README.md`

**Interfaces:**
- Consumes: the `api:check` script of Task 5.

- [ ] **Step 1: Add a CI step** after install and before build: `yarn api:generate` then `git diff --exit-code -- src/api/generated`. Keep the pinned action SHAs of the file. The step must not need the backend repository.
- [ ] **Step 2: Test the step.** Edit one generated file by hand, run the two commands, and see the failure. Revert the edit. Run them again. Expected: no diff.
- [ ] **Step 3: Document.** In `README.md`: how to update the API types (`yarn api:sync && yarn api:generate`, then commit), and that nobody edits `src/api/generated/` by hand.
- [ ] **Step 4: Run all checks.** `yarn fmt:check && yarn typecheck && yarn test && yarn build`. Expected: all pass.
- [ ] **Step 5: Commit and open the pull request.** `git commit -am "ci: check that the generated API code is current"`.

---

## Self-review notes

- Spec coverage: backend structs (Task 2), `ErrorCode` single source (Task 2), exact error lists and cookie scheme (Task 3), spec output, no-database writer, drift test (Task 4), generator, pinned version, throw semantics (Task 5), cache logic kept and types deleted (Task 6), typed error map (Task 7), frontend drift check (Task 8), shape tests first (Task 1).
- Names the later tasks use match the Interfaces blocks: `ErrorCode`, `ErrorBody`, `spec()`, `configureApiClient`, `api:sync`, `api:generate`, `api:check`.
- The generated function and type names are not known until Task 5 runs. Tasks 6 and 7 tell the builder to read them first.
