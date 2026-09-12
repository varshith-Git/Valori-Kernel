use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use tower::ServiceExt;
use valori_node::shared::SharedHost;

const ADMIN: &str = "operator-token-00000000000000000000000";
const A: &str = "project-a-token-0000000000000000000000";
const B: &str = "project-b-token-0000000000000000000000";
const ID_A: &str = "00000000-0000-0000-0000-000000000001";
const ID_B: &str = "00000000-0000-0000-0000-000000000002";

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 8 << 20).await.unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}
fn data(id: &str, path: &str) -> String {
    format!("/shared/projects/{id}/node{path}")
}
async fn create(app: &Router, id: &str, token: &str) {
    let result = call(
        app,
        "PUT",
        &format!("/shared/projects/{id}"),
        ADMIN,
        json!({"worker_auth_token":token,"max_records":100}),
    )
    .await;
    assert!(result.0.is_success(), "{result:?}");
}
async fn seed(app: &Router, id: &str, token: &str, value: f64) -> u64 {
    let collection = call(
        app,
        "POST",
        &data(id, "/v1/namespaces"),
        token,
        json!({"name":"documents","dimension":2,"metric":"squared_l2"}),
    )
    .await;
    assert!(collection.0.is_success(), "{collection:?}");
    let record = call(
        app,
        "POST",
        &data(id, "/v1/records"),
        token,
        json!({"collection":"documents","values":[value,0.0]}),
    )
    .await;
    assert!(record.0.is_success(), "{record:?}");
    record.1["id"].as_u64().unwrap()
}

#[tokio::test]
async fn shared_projects_isolate_names_records_graph_metadata_and_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let host = SharedHost::open(dir.path(), ADMIN, 10).unwrap();
    let app = host.router();
    create(&app, ID_A, A).await;
    create(&app, ID_B, B).await;
    let a = seed(&app, ID_A, A, 1.0).await;
    let b = seed(&app, ID_B, B, 9.0).await;
    assert_eq!(a, b, "record IDs may overlap across isolated engines");
    for (id, token, value) in [(ID_A, A, 1.0), (ID_B, B, 9.0)] {
        let result = call(
            &app,
            "GET",
            &data(id, &format!("/v1/records/{a}?collection=documents")),
            token,
            Value::Null,
        )
        .await;
        assert_eq!(result.0, StatusCode::OK, "{result:?}");
        assert_eq!(result.1["vector"][0], value);
    }
    assert_eq!(
        call(&app, "GET", &data(ID_B, "/v1/namespaces"), A, Value::Null)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, "GET", &data(ID_B, "/health"), A, Value::Null)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/shared/projects/{ID_B}"),
            A,
            Value::Null
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let node = call(
        &app,
        "POST",
        &data(ID_A, "/v1/graph/node"),
        A,
        json!({"collection":"documents","kind":0,"label":"private"}),
    )
    .await;
    assert!(node.0.is_success(), "{node:?}");
    let graph_a = call(
        &app,
        "GET",
        &data(ID_A, "/v1/graph/nodes?collection=documents"),
        A,
        Value::Null,
    )
    .await;
    let graph_b = call(
        &app,
        "GET",
        &data(ID_B, "/v1/graph/nodes?collection=documents"),
        B,
        Value::Null,
    )
    .await;
    assert_ne!(graph_a.1, graph_b.1);
    let meta = call(
        &app,
        "POST",
        &data(ID_A, "/v1/memory/meta/set"),
        A,
        json!({"target_id":"private","metadata":{"secret":"a"}}),
    )
    .await;
    assert!(meta.0.is_success(), "{meta:?}");
    let other_meta = call(
        &app,
        "GET",
        &data(ID_B, "/v1/memory/meta/get?target_id=private"),
        B,
        Value::Null,
    )
    .await;
    assert!(!other_meta.1.to_string().contains("secret"));
}

#[tokio::test]
async fn shared_restart_stop_and_delete_preserve_other_projects() {
    let dir = tempfile::tempdir().unwrap();
    let before;
    {
        let host = SharedHost::open(dir.path(), ADMIN, 10).unwrap();
        let app = host.router();
        create(&app, ID_A, A).await;
        create(&app, ID_B, B).await;
        seed(&app, ID_A, A, 1.0).await;
        seed(&app, ID_B, B, 9.0).await;
        before = call(&app, "GET", &data(ID_B, "/v1/proof/state"), B, Value::Null)
            .await
            .1;
        assert_eq!(
            call(
                &app,
                "PUT",
                &format!("/shared/projects/{ID_A}/active"),
                ADMIN,
                json!({"active":false})
            )
            .await
            .0,
            StatusCode::NO_CONTENT
        );
    }
    let host = SharedHost::open(dir.path(), ADMIN, 10).unwrap();
    let app = host.router();
    assert_eq!(
        call(&app, "GET", &data(ID_A, "/health"), A, Value::Null)
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(&app, "GET", &data(ID_B, "/v1/proof/state"), B, Value::Null)
            .await
            .1,
        before
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/shared/projects/{ID_A}"),
            ADMIN,
            Value::Null
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&app, "GET", &data(ID_A, "/health"), A, Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "GET", &data(ID_B, "/v1/proof/state"), B, Value::Null)
            .await
            .1,
        before
    );
}

#[tokio::test]
async fn shared_registration_is_idempotent_bounded_and_rejects_corruption() {
    let dir = tempfile::tempdir().unwrap();
    {
        let host = SharedHost::open(dir.path(), ADMIN, 1).unwrap();
        let app = host.router();
        create(&app, ID_A, A).await;
        create(&app, ID_A, A).await;
        let conflict = call(
            &app,
            "PUT",
            &format!("/shared/projects/{ID_A}"),
            ADMIN,
            json!({"worker_auth_token":B,"max_records":100}),
        )
        .await;
        assert_eq!(conflict.0, StatusCode::CONFLICT);
        let full = call(
            &app,
            "PUT",
            &format!("/shared/projects/{ID_B}"),
            ADMIN,
            json!({"worker_auth_token":B,"max_records":100}),
        )
        .await;
        assert_eq!(full.0, StatusCode::CONFLICT);
        seed(&app, ID_A, A, 1.0).await;
    }
    std::fs::write(dir.path().join(ID_A).join("events.log"), b"corrupted").unwrap();
    assert!(SharedHost::open(dir.path(), ADMIN, 10).is_err());
}

#[tokio::test]
async fn shared_export_import_preserves_audit_and_survives_dedicated_restart() {
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use valori_node::{
        config::NodeConfig, engine::Engine, server::build_router, EngineFromNodeConfig,
    };
    let source = tempfile::tempdir().unwrap();
    let host = SharedHost::open(source.path(), ADMIN, 10).unwrap();
    let app = host.router();
    create(&app, ID_A, A).await;
    seed(&app, ID_A, A, 3.0).await;
    let before = call(&app, "GET", &data(ID_A, "/v1/proof/state"), A, Value::Null)
        .await
        .1;
    call(
        &app,
        "PUT",
        &format!("/shared/projects/{ID_A}/active"),
        ADMIN,
        json!({"active":false}),
    )
    .await;
    let (status, bundle) = call(
        &app,
        "GET",
        &format!("/shared/projects/{ID_A}/export"),
        ADMIN,
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{bundle}");
    let destination = tempfile::tempdir().unwrap();
    let mut cfg = NodeConfig::default();
    cfg.event_log_path = Some(destination.path().join("events.log"));
    cfg.snapshot_path = Some(destination.path().join("snapshot.bin"));
    cfg.wal_path = None;
    {
        let state = Arc::new(RwLock::new(Engine::new(&cfg)));
        let target = build_router(state, Some(A.into()), None);
        let mut tampered = bundle.clone();
        tampered["state_hash"] = json!("wrong");
        assert_eq!(
            call(&target, "POST", "/internal/shared-import", A, tampered)
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            call(
                &target,
                "POST",
                "/internal/shared-import",
                B,
                bundle.clone()
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
        let result = call(
            &target,
            "POST",
            "/internal/shared-import",
            A,
            bundle.clone(),
        )
        .await;
        assert_eq!(result.0, StatusCode::OK, "{result:?}");
        assert_eq!(
            call(&target, "POST", "/internal/shared-import", A, bundle)
                .await
                .0,
            StatusCode::OK
        );
        assert_eq!(
            call(&target, "GET", "/v1/proof/state", A, Value::Null)
                .await
                .1,
            before
        );
    }
    let mut engine = Engine::new(&cfg);
    engine.try_recover();
    let target = build_router(Arc::new(RwLock::new(engine)), Some(A.into()), None);
    assert_eq!(
        call(&target, "GET", "/v1/proof/state", A, Value::Null)
            .await
            .1,
        before
    );
}

#[tokio::test]
async fn shared_data_credentials_cannot_access_worker_administration() {
    let dir = tempfile::tempdir().unwrap();
    let host = SharedHost::open(dir.path(), ADMIN, 10).unwrap();
    let app = host.router();
    create(&app, ID_A, A).await;
    for path in [
        "/metrics",
        "/v1/keys",
        "/v1/storage/snapshots",
        "/v1/snapshot/upload",
        "/internal/shared-import",
    ] {
        assert_eq!(
            call(&app, "POST", &data(ID_A, path), A, Value::Null)
                .await
                .0,
            StatusCode::FORBIDDEN,
            "{path}"
        );
    }
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/shared/projects/{ID_A}/export"),
            A,
            Value::Null
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, "GET", &data(ID_A, "/health"), ADMIN, Value::Null)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}
