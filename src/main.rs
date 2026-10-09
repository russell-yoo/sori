mod config;

use crate::config::load_config;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router, middleware};
use clap::Parser;
use reqwest::header;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[derive(Deserialize)]
struct NotificationRequest {
    message: String,
    destination: String,
    bot_name: Option<String>,
}

#[derive(Serialize)]
struct NotificationResponse {
    message_id: String,
    destination: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Serialize)]
struct DiscordWebhookRequest {
    content: String,
    username: String,
    allowed_mentions: AllowedMentions,
}

#[derive(Serialize)]
struct AllowedMentions {
    parse: Vec<String>,
}

#[derive(Deserialize)]
struct DiscordMessage {
    id: String,
}

struct AppState {
    destinations: HashMap<String, String>,
    http: reqwest::Client,
    api_token: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let app_config = load_config(&cli.config)?;
    let destinations = app_config.destinations;

    println!("Loaded {} destinations", destinations.len());

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .retry(reqwest::retry::never())
        .build()?;

    let api_token = app_config.api_token;

    let state = Arc::new(AppState {
        destinations,
        http,
        api_token,
    });

    let app = Router::new()
        .route("/health", get(health))
        .route(
            "/notifications",
            post(create_notification).route_layer(middleware::from_fn_with_state(
                Arc::clone(&state),
                require_api_token,
            )),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(app_config.bind_addr).await?;

    println!("Server is listening on {}", listener.local_addr()?);

    axum::serve(listener, app).await?;

    Ok(())
}

async fn health() -> &'static str {
    "ok"
}

async fn create_notification(
    State(state): State<Arc<AppState>>,
    Json(request): Json<NotificationRequest>,
) -> Result<Json<NotificationResponse>, (StatusCode, Json<ErrorResponse>)> {
    let webhook_url = match state.destinations.get(&request.destination) {
        Some(url) => url,
        None => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!("Invalid destination: {}", request.destination),
                }),
            ));
        }
    };

    let payload = DiscordWebhookRequest {
        content: request.message,
        username: request.bot_name.unwrap_or_else(|| String::from("Sori")),
        allowed_mentions: AllowedMentions { parse: Vec::new() },
    };

    let sent = send_discord(&state.http, webhook_url, &payload)
        .await
        .map_err(|error| {
            let status = if error.is_timeout() {
                StatusCode::GATEWAY_TIMEOUT
            } else {
                StatusCode::BAD_GATEWAY
            };

            (
                status,
                Json(ErrorResponse {
                    error: String::from("Discord delivery could not be confirmed"),
                }),
            )
        })?;

    Ok(Json(NotificationResponse {
        message_id: sent.id,
        destination: request.destination,
    }))
}

async fn send_discord(
    client: &reqwest::Client,
    webhook_url: &str,
    payload: &DiscordWebhookRequest,
) -> Result<DiscordMessage, reqwest::Error> {
    let response = client
        .post(webhook_url)
        .query(&[("wait", true)])
        .json(payload)
        .send()
        .await?
        .error_for_status()?;

    response.json::<DiscordMessage>().await
}

#[derive(Parser)]
struct Cli {
    #[arg(long, default_value = "config.yaml", value_name = "FILE")]
    config: PathBuf,
}

async fn require_api_token(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    let Some(expected_token) = state.api_token.as_deref() else {
        return next.run(request).await;
    };

    let authorized = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split_once(' '))
        .is_some_and(|(scheme, token)| {
            scheme.eq_ignore_ascii_case("Bearer") && token.trim_start_matches(' ') == expected_token
        });

    if !authorized {
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Bearer")],
            Json(ErrorResponse {
                error: String::from("Invalid or Missing API token"),
            }),
        )
            .into_response();
    }
    next.run(request).await
}
