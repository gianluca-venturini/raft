/** Calculate the destination RPC url based on the id */
pub fn calculate_rpc_server_dst(id: &str) -> String {
    // For now keep it simple and assume it's a 0-based integer
    let id_int = id.parse::<u32>().unwrap();
    format!("http://[::1]:{}", 50000 + id_int)
}