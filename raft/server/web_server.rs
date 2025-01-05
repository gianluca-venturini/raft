use actix_web::{web, App, HttpResponse, HttpServer};
use serde::Deserialize;
use std::env;
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;

use crate::state;

#[derive(Deserialize)]
struct GetRequest {
    key: String,
}

async fn get_variable(
    state: web::Data<Arc<AsyncMutex<state::State>>>,
    query: web::Query<GetRequest>,
) -> HttpResponse {
    let s = state.lock().await;
    let var = s.state_machine.vars.get(&query.key);
    if let Some(ref variable) = var {
        println!("Variable get: {} = {}", query.key, variable);
        HttpResponse::Ok().json(variable)
    } else {
        println!("Variable get: {} not set", query.key);
        HttpResponse::NotFound().body("Variable not set")
    }
}

#[derive(Deserialize)]
struct SetRequest {
    key: String,
    value: i32,
}

async fn set_variable(
    state: web::Data<Arc<AsyncMutex<state::State>>>,
    body: web::Json<SetRequest>,
) -> HttpResponse {
    state
        .lock()
        .await
        .state_machine
        .vars
        .insert(body.key.clone(), body.value);
    println!("Variable set: {} = {}", body.key, body.value);
    let mut response: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    response.insert(&"state", "ok");
    HttpResponse::Ok().json(response)
}

/** Retrieve a summary of the state of raft node 
 * only use this for debugging purposes
 */
async fn get_state(state: web::Data<Arc<AsyncMutex<state::State>>>) -> HttpResponse {
    let s = state.lock().await;
    let mut response = std::collections::HashMap::new();
    response.insert("role", &s.role);
    HttpResponse::Ok().json(response)
}

pub async fn start_web_server(
    state: Arc<AsyncMutex<state::State>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let port = env::var("PORT").expect("PORT environment variable is not set or cannot be read");
    let server = HttpServer::new(move || {
        let state = state.clone();
        App::new()
            .app_data(web::Data::new(state))
            .route("/variable", web::get().to(get_variable))
            .route("/variable", web::post().to(set_variable))
            .route("/state", web::get().to(get_state))
    })
    .bind(format!("localhost:{}", port))?
    .run();

    println!("Web server started");

    let send_future = async move { server.await };
    send_future.await?;

    Ok(())
}
