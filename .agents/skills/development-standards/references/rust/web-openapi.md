# Rust Axum OpenAPI Web Server

## Contents

- [Crates](#crates)
- [Layered Architecture](#layered-architecture)
- [Errors](#errors)
- [Routers](#routers)

## Crates

This page covers only the Axum + OpenAPI HTTP layer. Foundational Rust crates we depend on here are documented separately

HTTP-layer crates used on this page:

- `axum` — HTTP server / router (Controller layer)
- `utoipa` — Compile-time, code-first OpenAPI generation
- `utoipa-axum` — `OpenApiRouter` — keeps `axum` routes and OpenAPI spec in sync
- `utoipa-swagger-ui` — Serves Swagger UI for the generated spec

### `utoipa` (+ `utoipa-axum`, `utoipa-swagger-ui`)

`utoipa` is a **code-first**, compile-time OpenAPI generator. Annotate handlers with `#[utoipa::path(...)]` and DTOs with `#[derive(ToSchema)]`; the spec is built from the same Rust types your API actually uses. [[1]](https://github.com/juhaku/utoipa)[[2]](https://docs.rs/utoipa/latest/utoipa/derive.ToSchema.html)

We pair it with two companions:

- **`utoipa-axum`** — provides `OpenApiRouter`, an `axum::Router`-shaped type that automatically registers any handler annotated with `#[utoipa::path]` into the generated spec, eliminating drift between the router definition and the docs. [[3]](https://docs.rs/utoipa-axum)[[4]](https://docs.rs/utoipa-axum/latest/utoipa_axum/router/struct.OpenApiRouter.html)
- **`utoipa-swagger-ui`** — boilerplate to serve Swagger UI from the generated spec. Supports `axum >= 0.7`. [[5]](https://crates.io/crates/utoipa-swagger-ui)

```toml
[dependencies]
axum = "0.8"
utoipa = { version = "5", features = ["axum_extras", "chrono", "uuid"] }
utoipa-axum = "0.2"
utoipa-swagger-ui = { version = "9", features = ["axum"] }
```

**Conventions:**

- Every Controller `Request` and `Response` derives `ToSchema` in addition to `Deserialize` / `Serialize`.
- Every handler is annotated with `#[utoipa::path(...)]` listing `responses`, `params`, `request_body`, and a `tag` matching the feature module name.
- Routes are composed with `utoipa_axum::router::OpenApiRouter` and `utoipa_axum::routes!` — never with raw `axum::Router::route` for documented endpoints (it bypasses spec collection).
- A single `#[derive(OpenApi)]` struct in `src/router.rs` defines `info`, `servers`, and security schemes. Per-feature schemas and handlers are merged from their `OpenApiRouter` fragments.
- Mount Swagger UI at `/swagger-ui` and the raw spec at `/api-docs/openapi.json` only when runtime configuration explicitly enables API docs.

## Layered Architecture

Every feature module is split into three layers. Data flows downward as `*Input`, results bubble back upward as `*Output`. Each layer owns its own error enum and its own data shapes — types never leak across layers.

```
Repository ← (RepositoryInput) ← UseCase ← (UseCaseInput) ← Controller ← (Request)
Repository → (RepositoryOutput) → UseCase → (UseCaseOutput) → Controller → (Response)
```

### Module layout

```
foo/
  repository/
    mod.rs
    input.rs
    output.rs
  use_case/
    mod.rs
    input.rs
    output.rs
  controller/
    mod.rs
    request.rs
    response.rs
  router.rs
  mod.rs
error.rs
router.rs
lib.rs
main.rs
```

- One folder per feature (`foo/`). Sub-folders per layer.
- `error.rs` at the crate root re-exports the top-level error type used by `main`.
- `router.rs` at the crate root composes per-feature routers into the application router.

### Repository

Contains all I/O (database, HTTP clients, filesystem). Cannot be unit-tested directly; instead, expose a trait and provide a real implementation plus test doubles.

- `FooRepository` — trait (the interface).
- `FooRepositoryImpl` — production implementation (struct).
- `FooRepositoryStub` — deterministic stub for tests (struct).
- `FooBarRepositoryInput`, `FooBarRepositoryOutput` — per-method I/O types in `input.rs` / `output.rs`.
- All methods return `Result<_, FooRepositoryError>` (defined with `thiserror`). See the **Errors** section below for variant conventions.

```rust
// `BoxFuture` is the standard alias defined in general.md.
pub trait FooRepository: Send + Sync + 'static {
	fn get_foo(
		&self,
		input: GetFooRepositoryInput,
	) -> BoxFuture<'_, Result<GetFooRepositoryOutput, FooRepositoryError>>;
}
```

The boxed-future form is the default trait shape across the org — see [`general.md` § _Async traits with `Arc<dyn>`_](general.md#async-traits-with-arcdyn) for the rationale, the `BoxFuture` definition, and when reaching for `#[async_trait::async_trait]` is OK instead.

### UseCase

Contains business logic. Pure, deterministic, fully unit-testable via injected repository test doubles.

- `FooUseCase` — struct holding `Arc<dyn FooRepository>`. Production and tests
  inject different implementations without changing the UseCase or State type.
- `FooBarUseCaseInput`, `FooBarUseCaseOutput` — per-method I/O types.
- All methods return `Result<_, FooUseCaseError>`. See the **Errors** section below for repository-to-use-case mapping rules.
- No HTTP, no SQL, no `axum`, no `serde` derives on these types.

```rust
pub struct FooUseCase {
	pub(crate) repository: Arc<dyn FooRepository>,
}

impl FooUseCase {
	#[tracing::instrument(skip(self))]
	pub async fn get_foo(&self, input: GetFooUseCaseInput)
		-> Result<GetFooUseCaseOutput, FooUseCaseError> { /* … */ }
}
```

### Controller

The public HTTP boundary. Implemented as **free functions**, not structs — `UseCase`s are injected through `axum`'s `State` extractor. [[6]](https://docs.rs/axum/latest/axum/extract/struct.State.html)

- Handlers live in `controller/mod.rs`; route composition lives in the feature's `router.rs`.
- `FooBarRequest`, `FooBarResponse` derive `Deserialize` / `Serialize` **and `utoipa::ToSchema`**.
- Every public handler is annotated with `#[utoipa::path(...)]` and registered through `utoipa_axum::routes!` so the OpenAPI spec is generated from the same definitions as the router.
- Map `FooUseCaseError` → HTTP status codes via `IntoResponse` on `FooControllerError`, and list every reachable status in the `responses(...)` block of `#[utoipa::path]`. See the **Errors** section below for the full pattern.

```rust
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

#[derive(Clone)]
pub struct AppState {
	pub foo_use_case: Arc<FooUseCase>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct GetFooResponse {
	pub id: String,
	pub name: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorResponse {
	pub error: String,
}

/// Get a single foo by id.
#[utoipa::path(
	get,
	path = "/foo/{id}",
	tag = "foo",
	params(("id" = String, Path, description = "Foo identifier")),
	responses(
		(status = 200, description = "Foo found", body = GetFooResponse),
		(status = 403, description = "Caller not eligible", body = ErrorResponse),
		(status = 404, description = "Foo not found", body = ErrorResponse),
		(status = 500, description = "Internal error", body = ErrorResponse),
	),
)]
#[tracing::instrument(skip(state))]
pub async fn get_foo(
	State(state): State<AppState>,
	Path(id): Path<String>,
) -> Result<Json<GetFooResponse>, FooControllerError> {
	let output = state.foo_use_case
		.get_foo(GetFooUseCaseInput { id })
		.await?;
	Ok(Json(output.into()))
}

// Feature router: returns the axum::Router with state already attached,
// plus the OpenAPI fragment generated for this feature's handlers.
pub async fn init_foo_router()
	-> Result<(axum::Router, utoipa::openapi::OpenApi), crate::error::Error>
{
	let repository = FooRepositoryImpl::new(/* deps */);
	let use_case = FooUseCase { repository: Arc::new(repository) };
	let state = AppState { foo_use_case: Arc::new(use_case) };

	let (router, api) = OpenApiRouter::new()
		.routes(routes!(get_foo))
		.with_state(state)
		.split_for_parts();

	Ok((router, api))
}
```

## Errors

Each layer owns its own error enum. Like data and types, errors must not leak across layers — they get translated as they bubble up.

### Principles

- One error enum per layer: `FooRepositoryError`, `FooUseCaseError`, `FooControllerError`.
- All defined with `thiserror`. `Result<_, FooXError>` is the only return shape for fallible methods.
- Only the Controller knows HTTP. UseCases and Repositories must never reference `axum`, `StatusCode`, or response bodies.
- Translate explicitly when an error carries business meaning; use `#[from]` only for genuinely opaque, internal failures.

### Repository errors

Repository errors describe I/O facts: missing rows, broken connections, deserialization failures.

```rust
#[derive(Debug, thiserror::Error)]
pub enum FooRepositoryError {
    #[error("foo not found")]
    NotFound,
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}
```

- Use `#[from]` for driver-level errors (`sqlx::Error`, `reqwest::Error`, `serde_json::Error`) — they carry no business meaning at this layer.
- Surface semantic outcomes (`NotFound`, `ConstraintViolated`, …) as their own variants so the UseCase can map them deliberately.

### UseCase errors

UseCase errors describe business outcomes: not eligible, quota exceeded, state transition disallowed.

```rust
#[derive(Debug, thiserror::Error)]
pub enum FooUseCaseError {
    #[error("foo not found")]
    NotFound,
    #[error("foo not eligible: {reason}")]
    NotEligible { reason: String },
    #[error(transparent)]
    Repository(#[from] FooRepositoryError),
}
```

- Map repository errors **explicitly** when they carry business meaning. Letting `FooRepositoryError::NotFound` reach the controller as `Repository(...)` turns a domain "not found" into a 500.

```rust
// Wrong: blanket bubble — "not found" becomes 500.
let foo = self.repository.get_foo(input).await?;

// Right: translate the semantic case explicitly.
let foo = self.repository
    .get_foo(input)
    .await
    .map_err(|e| match e {
        FooRepositoryError::NotFound => FooUseCaseError::NotFound,
        other => FooUseCaseError::Repository(other),
    })?;
```

- Reserve `Repository(#[from] FooRepositoryError)` for _genuinely_ internal failures (DB down, serialization broken) — those map to 5xx in the controller.

### Controller errors

Controller errors describe protocol outcomes. The controller is the single layer that decides HTTP status, response shape, and what gets logged.

```rust
#[derive(Debug, thiserror::Error)]
pub enum FooControllerError {
    #[error("invalid request: {0}")]
    BadRequest(String),
    #[error(transparent)]
    UseCase(#[from] FooUseCaseError),
}

impl axum::response::IntoResponse for FooControllerError {
    fn into_response(self) -> axum::response::Response {
        use axum::http::StatusCode;
        let (status, message) = match &self {
            Self::BadRequest(_) => (StatusCode::BAD_REQUEST, "invalid request"),
            Self::UseCase(FooUseCaseError::NotFound) =>
                (StatusCode::NOT_FOUND, "foo not found"),
            Self::UseCase(FooUseCaseError::NotEligible { .. }) =>
                (StatusCode::FORBIDDEN, "foo not eligible"),
            Self::UseCase(FooUseCaseError::Repository(_)) =>
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error"),
        };
        tracing::error!(error = ?self, "request failed");
        let body = ErrorResponse { error: message.into() };
        (status, axum::Json(body)).into_response()
    }
}
```

- Match every reachable variant. New variants must update both the `match` arm and the `#[utoipa::path(... responses(...))]` block — these two stay in sync as one unit.
- Log at this boundary, not earlier. Lower layers return errors silently; the controller is the single place that decides what makes it into logs.
- Never include raw repository or use-case messages in the response body without filtering — they may leak SQL, file paths, or PII.

### OpenAPI alignment

Every reachable controller-error variant must appear in the handler's `#[utoipa::path(... responses(...))]` list. Treat the `match` in `IntoResponse` and the `responses(...)` block as one unit: change them together, review them together.

```rust
#[utoipa::path(
    get,
    path = "/foo/{id}",
    tag = "foo",
    params(("id" = String, Path, description = "Foo identifier")),
	responses(
		(status = 200, description = "Foo found", body = GetFooResponse),
		(status = 404, description = "Foo not found", body = ErrorResponse),
		(status = 403, description = "Caller not eligible", body = ErrorResponse),
		(status = 500, description = "Internal error", body = ErrorResponse),
	),
)]
```

An OpenAPI snapshot test catches accidental spec changes, but it cannot compare
the annotation with `IntoResponse`. Controller tests must assert the status and
public error body for each reachable error variant.

### Crate-root error (`src/error.rs`)

The crate-root `Error` is what `main` returns. It wraps router-init failures, config errors, and (where useful) controller errors — not a replacement for the per-feature enums.

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("router init failed: {0}")]
    RouterInit(String),
    #[error("config error: {0}")]
    Config(#[from] config::ConfigError),
}
```

### Testing

- **Repository**: integration tests only — assert real driver errors map to the right `FooRepositoryError` variant.
- **UseCase**: feed each `FooRepositoryError` variant via `FooRepositoryStub` and assert the resulting `FooUseCaseError`. This is the only place the repo-to-use-case mapping is verified.
- **Controller**: drive `axum::Router` with `tower::ServiceExt::oneshot`, inject a `FooUseCase` over a stub repository, and assert the HTTP status for each `FooControllerError` variant.
- **OpenAPI**: initialize the router with API docs enabled, request `/api-docs/openapi.json`, and snapshot that response. It contains the fully merged root and feature specification. Controller tests separately verify each error's status and public body.

## Routers

Routers are the glue between Controllers and the running `axum` application. We use a two-tier pattern:

- **Per-feature router** (`src/<feature>/router.rs`) — builds the feature's `Repository` → `UseCase` → `State` chain, registers handlers via `OpenApiRouter::routes(routes!(...))`, attaches state with `.with_state(...)`, and returns `(axum::Router, utoipa::openapi::OpenApi)` via `split_for_parts()`.
- **Root router** (`src/router.rs`) — owns the global `#[derive(OpenApi)]` doc (title, version, contact, license), merges every feature's router and OpenAPI fragment, mounts Swagger UI when enabled, and applies cross-cutting `tower_http` layers.

### Why `(axum::Router, utoipa::openapi::OpenApi)`?

Each feature owns its own `State` type. Returning a finished `axum::Router` (state already injected) plus the OpenAPI fragment lets the root router merge routers from features with **incompatible state types** — something `OpenApiRouter<S>` cannot do directly.

### Per-feature router (`src/<feature>/router.rs`)

```rust
//! Initializes and returns the feature's axum router.
use utoipa_axum::{router::OpenApiRouter, routes};

#[derive(Clone)]
pub struct FooState {
	pub foo_use_case: std::sync::Arc<crate::foo::use_case::FooUseCase>,
}

pub async fn init_foo_router()
	-> Result<(axum::Router, utoipa::openapi::OpenApi), crate::error::Error>
{
	// 1. Build the dependency graph: Repository → UseCase → State.
	let state = /* FooState { ... } */;

	// 2. Register one handler per `routes!(...)` call.
	let (router, api) = OpenApiRouter::new()
		.routes(routes!(crate::foo::controller::list_foos))
		.routes(routes!(crate::foo::controller::get_foo))
		// …one .routes(routes!(...)) per handler
		.with_state(state)
		.split_for_parts();

	Ok((router, api))
}
```

**Rules:**

- One `init_<feature>_router()` per feature module. Always `async`, always returns `Result<(axum::Router, utoipa::openapi::OpenApi), crate::error::Error>`.
- Construct the dependency graph (`Repository` → `UseCase` → `State`) inside this function. Keep the state `Clone` by wrapping shared services in `Arc`.
- Register one handler per `routes!(...)` call — do not group multiple handlers in a single `routes!` invocation; it makes diffs and grep noisier.
- Always finish with `.with_state(state).split_for_parts()` so the returned `axum::Router` is fully self-contained.
- Do **not** apply global middleware (compression, panic-catching, tracing, CORS) here — those belong on the root router.

### Root router (`src/router.rs`)

The root router defines `ApiDoc`, merges per-feature routers and their OpenAPI fragments using `OpenApi::merge_from`, mounts Swagger UI when enabled, and applies global middleware. The process entry point builds it once and passes it to the server adapter.

```rust
//! Initializes and returns the root axum router.
use utoipa::OpenApi;
use utoipa_axum::{router::OpenApiRouter, routes};

#[derive(OpenApi)]
#[openapi(
	info(
		title = "<service-name>",
		version = "1.0.0",
		description = "<short API description>",
		contact(name = "<owner>", email = "<owner@example.com>"),
		license(name = "<license>")
	)
)]
struct ApiDoc;

/// Initializes the axum router.
pub async fn init_router(
	expose_api_docs: bool,
) -> Result<axum::Router, crate::error::Error> {
	// 1. Build each feature router (returns axum::Router + OpenAPI fragment).
	let (foo_router, foo_api) = crate::foo::router::init_foo_router().await?;
	// …one let-binding per feature module

	// 2. Register any root-level routes (health, version, …).
	let (router, root_api) = OpenApiRouter::new()
		.routes(routes!(handle_health_check))
		.split_for_parts();

	// 3. Merge every OpenAPI fragment into the root ApiDoc.
	let api = ApiDoc::openapi()
		.merge_from(root_api)
		.merge_from(foo_api);

	// 4. Compose the final axum::Router. Production configuration passes
	// false; local development and the OpenAPI snapshot test pass true.
	let app = router.merge(foo_router);
	let app = if expose_api_docs {
		app.merge(
			utoipa_swagger_ui::SwaggerUi::new("/swagger-ui")
				.url("/api-docs/openapi.json", api),
		)
	} else {
		app
	};
	let app = app
		.layer(tower_http::compression::CompressionLayer::new())
		.layer(tower_http::catch_panic::CatchPanicLayer::new());

	Ok(app)
}

// Example root-level handler. Real handlers live in feature modules.
#[derive(utoipa::ToSchema, serde::Serialize)]
struct HealthStatus { status: String }

#[utoipa::path(get, path = "/api/health",
	responses((status = 200, description = "OK", body = HealthStatus)))]
async fn handle_health_check() -> impl axum::response::IntoResponse {
	axum::Json(HealthStatus { status: "ok".into() })
}
```

**Rules:**

- `ApiDoc` lives only in `src/router.rs` and owns global metadata such as title, version, description, contact, and license. Root and feature handlers contribute paths and schemas through their `OpenApiRouter` fragments.
- Build the router once in the process entry point and clone the resulting `axum::Router` where the server adapter requires ownership. Do not add a global cache; explicit construction keeps runtime configuration and tests independent.
- Build root-local routes (e.g. `handle_health_check`) with `OpenApiRouter::new().routes(routes!(...)).split_for_parts()` exactly like a feature module would, then `.merge_from(...)` the resulting fragment into `ApiDoc::openapi()`.
- Merge OpenAPI fragments in this order: root metadata → root-local auto-generated → each feature. Pass the merged `api` to `SwaggerUi::url(...)`.
- Gate API docs with explicit runtime configuration. Production passes `false`; local development and the OpenAPI snapshot test pass `true`. When enabled, mount Swagger UI at `/swagger-ui` and the raw spec at `/api-docs/openapi.json`.
- Global `tower_http` layers belong here, not on feature routers. Standard set:
  - `CompressionLayer` with `deflate`, `gzip`, `br`, `zstd` enabled.
  - `CatchPanicLayer` to convert panics into `500` responses instead of dropping the connection.
  - Add `TraceLayer`, `CorsLayer`, and `RequestBodyLimitLayer` here as the service grows.
- `init_router(expose_api_docs)` returns an owned `axum::Router`. Call it once from the process entry point; router clones are cheap.

### Testing rules of thumb

- `Repository`: integration-tested only (real DB / HTTP). No unit tests.
- `UseCase`: 100% unit-tested with `FooRepositoryStub`. Cover happy path + every error variant.
- `Controller`: tested with `axum::Router` + `tower::ServiceExt::oneshot`, injecting an `AppState` whose `UseCase` wraps a stub repository.
- **OpenAPI**: call `init_router(true)`, request `/api-docs/openapi.json`, and compare the JSON response with a checked-in fixture so drift in the fully merged specification fails CI.
