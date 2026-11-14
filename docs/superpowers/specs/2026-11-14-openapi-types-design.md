# OpenAPI types for the web frontend: design

Date: 2026-11-14
Repositories: `dearest.rsvp-api` (backend), `dearest.rsvp-web` (frontend)

## Intent

The Rust backend is the source of truth for the API. The frontend must get
its types and its API calls from the backend, not from hand-written code.
After this work, a change to an API shape breaks the frontend type check.
It does not break production.

Today the frontend has about 35 lines of hand-written types in
`src/api/types.ts` and hand-written calls in `src/api/*.ts`. Nothing
makes them match the backend.

## Goals

- The backend makes an OpenAPI 3.1 spec from its Rust code.
- The frontend generates types, a typed client, and TanStack Query options from that spec.
- The spec covers every route: path, method, request body, success response, and error responses.
- Each error code in `AppError` appears in the spec, so the frontend has a type for it.
- CI fails in the backend if the committed spec is not current.
- CI fails in the frontend if the committed generated code is not current.
- The frontend deletes the hand-written types in `src/api/types.ts` and the hand-written calls that the generated client replaces.

## Non-goals

- No runtime validation of responses in the browser (no Zod). Types only.
- No Swagger UI or public docs page. The backend may serve one later.
- No change to the API behavior, URLs, status codes, or JSON shapes. The generated types must describe the API as it is now.
- No automatic publish of the spec to a package registry.

## Anti-goals

The work is a failure, even if it compiles, if one of these is true:

- The spec says one thing and the handler does another. Example: a response struct that does not match what the handler returns.
- A developer must edit generated code by hand.
- The frontend loses the current handling of the session cookie, the 401 flow, or the `errorMessage` copy.
- The `json!` responses stay in the handlers, and the spec describes different types in a separate place.

## Constraints

- Backend: axum 0.8, sea-orm 2, Rust edition 2024. Frontend: React 19, TanStack Query 5, TanStack Router, Vite+, TypeScript 6, yarn 4.
- The session is an HttpOnly cookie. Every request needs `credentials: "include"`.
- The backend rejects a write request from an origin that is not in `cors_allowed_origins`. The client must not remove the `Origin` header the browser sends.
- Response bodies are built today with `json!({...})` and `serde_json::Value`. Only request bodies are real structs. The spec needs a named struct for each response.
- The error body is `{ "error": "<code>" }`. `AppError` is an enum with a code string for each variant (see `AppError::parts` in `src/error.rs`).
- Both repositories are on an up to date `main` with no uncommitted work. Work in each repository starts on a new branch from `main`.

## Approaches

Backend, how the spec is made:

1. **`utoipa` + `utoipa-axum` (code-first).** Derive `ToSchema` on structs. Annotate handlers with `#[utoipa::path]`. Cost: annotation on each handler.
2. **`aide` (code-first, axum-native).** Uses `schemars`. Less annotation. Cost: a smaller community and its own router and extractor wrappers.
3. **Spec-first.** Write `openapi.yaml` by hand and generate server types. Cost: handlers stop being the source of truth.

Frontend, how the spec is used:

1. **`@hey-api/openapi-ts`.** Generates types, a flat typed SDK, and a TanStack Query plugin that makes `queryOptions` and `mutationOptions`.
2. **`orval`.** Generates clients and hooks. More code, and it overlaps with the hooks that exist.
3. **`openapi-typescript` + `openapi-fetch`.** Types and a thin client only. No query options.

**Chosen by Will:** `utoipa` + `utoipa-axum` for the backend, and a generator that makes
a client and TanStack Query integration for the frontend.

**Proposed by Claude, confirmed by Will:** `@hey-api/openapi-ts`, with the exact
version pinned. Reason: its output is plain and readable, its query plugin matches
TanStack Query 5, and it leaves cache keys and invalidation to the app.
Risk: it is pre-1.0 and a minor version can change the output. The pin and the
committed generated code reduce this risk.

## Design

### Backend

1. Add `utoipa` and `utoipa-axum` to `Cargo.toml`. Enable the `uuid` and `chrono` features.
2. Replace each `json!` response with a named struct that derives `Serialize` and `ToSchema`. The JSON the struct produces must equal the JSON of today. The existing integration tests in `tests/` check this and must pass without change.
   Structs needed (names are a proposal): `Me`, `MeUser`, `MembershipSummary`, `Tenant`, `Member`, `Invite`, `InvitePreview`, `Health`, and the small bodies for create invite, accept invite, and other routes that return JSON.
3. Add `#[derive(ToSchema)]` to the existing request body structs.
4. Make an `ErrorBody` struct with an `error` field. Its type is an enum of the codes. The enum comes from `AppError` so the list of codes has one source. A route lists the error statuses it can return.
5. Annotate each handler with `#[utoipa::path]`. Use `utoipa-axum` (`OpenApiRouter`, `routes!`) so the route and the spec entry stay together.
6. Declare the session cookie as a security scheme in the spec. Mark the routes that need a session.
7. `ApiJson<T>` is a custom extractor. It needs a `body = T` hint in the path annotation. The builder checks that the spec shows the right body.
8. Add a way to write the spec to `openapi.json` in the repository root. A binary or an `--emit-openapi` flag on `main` are both fine. A test compares the committed file to the generated spec and fails if they differ. The failure message says how to regenerate the file.

### Frontend

1. Add `@hey-api/openapi-ts` as a dev dependency. Pin the exact version.
2. Add `openapi-ts.config.ts`. It reads the spec, writes to `src/api/generated/`, and uses the `@hey-api/client-fetch` client and the `@tanstack/react-query` plugin.
3. Configure the generated client in one place: base URL from `VITE_API_BASE_URL`, `credentials: "include"`, and an error mapper that keeps today's `ApiError` and `errorMessage` behavior.
4. Add scripts to `package.json`: one to generate (`yarn api:generate`) and one to check the committed output is current.
5. Source of the spec: a path to the backend repo (sibling directory) by default. The path is a config value. See "Left to the builder".
6. Move each call in `src/api/auth.ts`, `invites.ts`, and `tenants.ts` to the generated functions or the generated query options. Keep the file names and the exported hook names the pages use now, so the pages need small changes only.
7. Delete `src/api/types.ts` when nothing imports it. The pages import types from the generated code.
8. The existing tests in `src/api/*.test.ts` must pass or change only where the call shape changes.

### CI

- Backend: the spec test above runs in the existing `ci.yml`.
- Frontend: a step runs the generator and fails if `git diff` shows a change in `src/api/generated/`.
- The frontend CI needs the spec. Options are in "Left to the builder".

## Order of work

1. Backend: response structs, with the current tests as proof the JSON did not change.
2. Backend: `utoipa` annotations and the spec output with its test.
3. Frontend: generator setup and the generated client.
4. Frontend: migrate calls, delete hand-written types.
5. CI checks in both repositories.

Each step ends with a passing build and test run in its repository.

## Left to the builder

- Exact struct and type names.
- How the frontend gets the spec in CI: a copy of `openapi.json` committed to the frontend, a download from the backend repository at a fixed ref, or a git submodule. The builder picks the simplest option that CI can run. It states the choice in the plan.
- Whether the spec writer is a binary, a flag, or a test helper.
- The exact `utoipa` feature flags.
- Whether the generated code uses the SDK functions directly or only the query options. The aim is the smallest change to the pages.
- File layout inside `src/api/generated/`.

## Risks

- `utoipa` does not see a response type that the handler hides behind `Value`. The fix is the struct step in the backend part. The risk is a struct that drifts from the handler. The test that compares the spec to the committed file does not catch this. The existing integration tests do, if they assert the JSON shape.
- The generator may change its output in a minor release. The pinned version and the generated-code check give protection.
- The backend and the frontend change in two repositories. Merge the backend branch first. The frontend branch needs the spec file from it.
