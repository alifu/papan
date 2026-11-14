//! A real GraphQL server for the integration tests (plan 06 §11): `async-graphql` on axum.
//! `/graphql` is open; `/secure/graphql` wants `Authorization: Bearer <SECURE_TOKEN>`.
//! `/slow` answers after five seconds. Every request's `Authorization` header is recorded.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use async_graphql::{EmptySubscription, Object, Schema, SimpleObject, ID};
use async_graphql_axum::GraphQL;
use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;

pub const SECURE_TOKEN: &str = "graphql-token-7731";

#[derive(SimpleObject, Clone)]
pub struct User {
    pub id: ID,
    pub name: String,
}

pub struct Query;

#[Object]
impl Query {
    /// The user with `id`.
    async fn user(&self, id: ID) -> User {
        User {
            name: format!("User {}", id.as_str()),
            id,
        }
    }

    async fn greeting(&self, name: Option<String>) -> String {
        format!("Hello, {}!", name.unwrap_or_else(|| "world".into()))
    }

    #[graphql(deprecation = "Use `user` instead.")]
    async fn me(&self) -> Option<User> {
        None
    }
}

pub struct Mutation;

#[Object]
impl Mutation {
    async fn create_user(&self, name: String) -> User {
        User {
            id: ID::from("new"),
            name,
        }
    }
}

pub type TestSchema = Schema<Query, Mutation, EmptySubscription>;

pub fn schema() -> TestSchema {
    Schema::build(Query, Mutation, EmptySubscription).finish()
}

pub struct GraphqlServer {
    pub addr: SocketAddr,
    seen: Arc<Mutex<Vec<Option<String>>>>,
}

impl GraphqlServer {
    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    /// The `Authorization` header of every request so far, in order.
    pub fn authorizations(&self) -> Vec<Option<String>> {
        self.seen.lock().unwrap().clone()
    }

    pub fn hits(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}

async fn record(
    State(seen): State<Arc<Mutex<Vec<Option<String>>>>>,
    request: Request,
    next: Next,
) -> Response {
    let authorization = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let secure = request.uri().path().starts_with("/secure/");
    let allowed = authorization.as_deref() == Some(&format!("Bearer {SECURE_TOKEN}"));
    seen.lock().unwrap().push(authorization);
    if secure && !allowed {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    next.run(request).await
}

/// Answers after five seconds (for cancel tests).
async fn slow() -> &'static str {
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    "late"
}

/// Starts the server on `127.0.0.1:0` in the current tokio runtime.
pub async fn spawn() -> GraphqlServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route_service("/graphql", GraphQL::new(schema()))
        .route_service("/secure/graphql", GraphQL::new(schema()))
        .route("/slow", axum::routing::post(slow))
        .layer(middleware::from_fn_with_state(Arc::clone(&seen), record));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    GraphqlServer { addr, seen }
}
