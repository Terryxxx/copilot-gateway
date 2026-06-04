use copilot_gateway::server::build_router;
use copilot_gateway::state::AppState;

#[tokio::test]
async fn health_route_responds() {
    let state = AppState::new(reqwest::Client::new(), "individual".into());
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let body = reqwest::get(format!("http://{addr}/"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(body.contains("copilot-gateway"));
}
