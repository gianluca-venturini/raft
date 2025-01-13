use actix_web::{web, App, HttpResponse, HttpServer};
use serde::Deserialize;
use serde_json::json;
use std::env;
use std::sync::Arc;
use tokio::sync::RwLock as AsyncRwLock;

use crate::state;
use crate::update::send_update_all;

#[derive(Deserialize)]
struct GetRequest {
    key: String,
}

async fn handle_not_leader(leader_id: &Option<String>) -> HttpResponse {
    let mut response: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    if let Some(ref leader_id) = leader_id {
        response.insert("leaderId", leader_id.as_str());
        println!("Current node not leader. Leader is {}", leader_id);
    } else {
        println!("Leader is unknown");
    }
    HttpResponse::PermanentRedirect().json(response)
}

async fn get_variable(
    state: web::Data<Arc<AsyncRwLock<state::State>>>,
    query: web::Query<GetRequest>,
) -> HttpResponse {
    let s = state.read().await;
    if s.role != state::Role::Leader {
        return handle_not_leader(&s.volatile.leader_id).await;
    }
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
    state: web::Data<Arc<AsyncRwLock<state::State>>>,
    body: web::Json<SetRequest>,
) -> HttpResponse {
    {
        let mut s = state.write().await;
        if s.role != state::Role::Leader {
            return handle_not_leader(&s.volatile.leader_id).await;
        }

        let entry = state::LogEntry {
            term: s.get_current_term(),
            command: state::Command::WriteVar {
                name: body.key.clone(),
                value: body.value,
            },
        };
        s.append_log_entry(entry);
    }

    send_update_all(state.get_ref().clone()).await;
    // TODO: do not return until the update has been sent to a majority of nodes and is committed

    println!("Variable set: {} = {}", body.key, body.value);
    let mut response: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    response.insert("state", "ok");
    HttpResponse::Ok().json(response)
}

/** Retrieve a summary of the state of raft node
 * only use this for debugging purposes
 */
async fn get_state(state: web::Data<Arc<AsyncRwLock<state::State>>>) -> HttpResponse {
    let s = state.read().await;
    let response = json!({
        "role": s.role,
        "log": s.get_log(),
    });
    HttpResponse::Ok().json(response)
}

pub async fn start_web_server(
    state: Arc<AsyncRwLock<state::State>>,
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
