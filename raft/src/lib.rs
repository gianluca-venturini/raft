use wasm_bindgen::prelude::*;

pub mod state;

#[wasm_bindgen]
pub fn greet(name: &str) -> String {
    format!("Hello, {}!", name)
}
