# Rust GraphQL Web Server Standards

GraphQL counterpart to [`web-openapi.md`](web-openapi.md). Read that file
first — layering, error-per-layer, and `Arc<dyn Repository>` conventions
from `general.md` all still apply here.

**Status note:** the only GraphQL implementation found in an audit of org
repos lives in a crate that has since been **superseded by REST +
OpenAPI/utoipa** (the pattern documented in `web-openapi.md`). Treat what
follows as "how we did it the one time we did," not as an actively
maintained, first-class alternative to the OpenAPI path. Default to
`web-openapi.md` for a new HTTP API; reach for this file only if a
consumer genuinely needs a GraphQL surface (e.g. a client that needs
flexible field selection across nested resources).

## Crate choice: code-first with `async-graphql`

```toml
[dependencies]
async-graphql = "7"
async-graphql-axum = "7"
```

Code-first, not schema-first — the schema is generated from Rust types
and resolver functions annotated with `async-graphql` macros, the same
philosophy as `utoipa`'s code-first OpenAPI generation.

Use the **`async-graphql-axum` extractor and response types**
(`GraphQLRequest` / `GraphQLResponse`) for the handler, not a hand-rolled
`serde_json::from_slice::<async_graphql::Request>` parse — the extractor
gets you the same integration `utoipa-axum` gives the OpenAPI path, with
less boilerplate and less room for a body-parsing bug:

```rust
use async_graphql::{EmptyMutation, EmptySubscription, Schema};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};

async fn graphql_handler(
    State(schema): State<Schema<Query, EmptyMutation, EmptySubscription>>,
    req: GraphQLRequest,
) -> GraphQLResponse {
    schema.execute(req.into_inner()).await.into()
}
```

## Schema composition

Merge one resolver struct per feature module into a single root query type
with `#[derive(async_graphql::MergedObject)]`, mirroring the per-feature
router pattern in `web-openapi.md`:

```rust
#[derive(async_graphql::MergedObject, Default)]
pub struct Query(FooQuery, BarQuery);
```

Start with `EmptyMutation` / `EmptySubscription` for a read-only API; add
`Mutation`/`Subscription` types only when the API actually needs them —
don't scaffold empty mutation/subscription surfaces speculatively.

Build the schema once in the process entry point and place a clone in Axum
state. `Schema` is already cheap to clone, so a separate global cache adds no
value.

## Layering: Repository → Service → Resolver, with a translation stage

GraphQL resolvers don't map cleanly onto `web-openapi.md`'s
Controller/UseCase naming — a resolver is closer to a Controller, but the
data usually needs an extra shape change GraphQL types don't share with
transport-layer DTOs. Use three stages instead of two:

```text
Record (raw external-API shape) → Entity (domain) → GraphQL type (SimpleObject)
```

```rust
// record/foo.rs — matches the upstream API's JSON shape exactly
pub struct FooRecord { /* ... */ }

// entity/foo.rs — domain shape, independent of any transport
pub struct FooEntity { /* ... */ }

// resolver/foo/query.rs — GraphQL-facing type
#[derive(async_graphql::SimpleObject)]
pub struct Foo { /* ... */ }
```

Repository traits use the same `Arc<dyn FooRepository>` plus
`FooRepositoryStub` test-double pattern as `web-openapi.md`. Production and
tests inject different implementations without changing the Service or
Resolver types.

## Lazy fields and the N+1 tradeoff

For a field that's expensive to compute and not always requested, use
`#[async_graphql::ComplexObject]` to resolve it lazily, only when a query
actually selects it, rather than eagerly populating it on every load:

```rust
#[derive(async_graphql::SimpleObject)]
#[graphql(complex)]
pub struct Foo {
    pub id: String,
    pub name: String,
    #[graphql(skip)]
    pub child_ids: Vec<String>,
}

#[async_graphql::ComplexObject]
impl Foo {
    async fn children(&self, ctx: &async_graphql::Context<'_>) -> Result<Vec<Child>, FooError> {
        let service = ctx.data::<Arc<FooService>>()?;
        service.get_children(&self.child_ids).await
    }
}
```

This is a per-field lazy fetch, not a dataloader-based batching solution —
it avoids fetching data nobody asked for, but doesn't by itself prevent
N+1 queries when a list of parents each resolve the same child relation.
Reach for [`async-graphql::dataloader`](https://async-graphql.github.io/async-graphql/en/dataloader.html)
if that pattern actually shows up under load; don't add it preemptively.

## Testing

Same shape as `web-openapi.md`'s controller tests: drive the schema
directly with `schema.execute(...)` against a `Query` built over a stub
repository/service, and assert on the returned `async_graphql::Response`'s
data and errors — no HTTP layer needed for this tier. Reserve
`tower::ServiceExt::oneshot` against the full `axum::Router` for a small
number of true end-to-end tests that also exercise the `graphql_handler`
extraction itself.
