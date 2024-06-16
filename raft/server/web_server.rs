use actix_web::{web, App, HttpResponse, HttpServer};
use std::env;
use std::sync::{Arc, RwLock};

use crate::state;

async fn get_variable(state: web::Data<Arc<RwLock<state::State>>>) -> HttpResponse {
    let s = state.read().unwrap();
    let var = s.state_machine.vars.get("key");
    if let Some(ref variable) = var {
        HttpResponse::Ok().json(variable)
    } else {
        HttpResponse::NotFound().body("Variable not set")
    }
}

async fn set_variable(state: web::Data<Arc<RwLock<state::State>>>) -> HttpResponse {
    state
        .write()
        .unwrap()
        .state_machine
        .vars
        .insert("key".to_string(), 42);
    HttpResponse::Ok().body("Variable set")
}

pub async fn start_web_server(
    state: Arc<RwLock<state::State>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let port = env::var("PORT").expect("PORT environment variable is not set or cannot be read");
    let server = HttpServer::new(move || {
        let state = state.clone();
        App::new()
            .app_data(web::Data::new(state))
            .route("/variable", web::get().to(get_variable))
            .route("/variable", web::post().to(set_variable))
    })
    .bind(format!("localhost:{}", port))?
    .run();

    println!("Web server started");

    let send_future = async move { server.await };
    send_future.await?;

    println!("server started");

    Ok(())
}
