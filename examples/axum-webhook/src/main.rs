use axum::{
    body::Bytes,
    http::{HeaderMap, StatusCode},
    routing::post,
    Router,
};
use payment_gateway_client::{verify_webhook, SIGNATURE_HEADER};

#[tokio::main]
async fn main() {
    let secret = std::env::var("GATEWAY_WEBHOOK_SECRET").expect("GATEWAY_WEBHOOK_SECRET");
    let app = Router::new().route(
        "/webhook",
        post(move |headers: HeaderMap, body: Bytes| {
            let secret = secret.clone();
            async move {
                let signature = headers.get(SIGNATURE_HEADER).and_then(|v| v.to_str().ok());
                match verify_webhook(&body, signature, &secret, 300) {
                    Ok(event) => {
                        println!(
                            "{} {}",
                            event["type"], event["data"]["payment"]["reference"]
                        );
                        StatusCode::OK
                    }
                    Err(_) => StatusCode::BAD_REQUEST,
                }
            }
        }),
    );
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".into());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}
