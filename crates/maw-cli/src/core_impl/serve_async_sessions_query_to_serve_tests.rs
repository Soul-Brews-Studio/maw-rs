#[derive(Deserialize)]
struct SessionsQuery {
    local: Option<bool>,
}

#[derive(Deserialize)]
struct CaptureQuery {
    target: Option<String>,
}

#[cfg(test)]
#[allow(clippy::redundant_closure_for_method_calls)]
mod serve_tests {
    include!("serve_async_serve_tests/01_fake_serve_delivery_to_response_json.rs");
    include!("serve_async_serve_tests/02_serve_peers_info_rout_7b5053_to_serve_peer_pubkey_c_3ce81f.rs");
    include!("serve_async_serve_tests/03_serve_o6_node_fallbac_673607_to_serve_o6_live_route_f12d93.rs");
    include!("serve_async_serve_tests/04_serve_o6_live_router_2f371d_to_serve_api_send_inbox_1ec0fd.rs");
    include!("serve_async_serve_tests/05_receiver_inbox_manife_b19988_to_receiver_inbox_targ_108179.rs");
    include!("serve_async_serve_tests/06_serve_api_send_inbox_0dd2ea_to_spawn_test_server.rs");
    include!("serve_async_serve_tests/07_serve_real_wire_accep_2ca6b6_to_serve_host_validati_2564e4.rs");
    include!("serve_async_serve_tests/08_serve_core_real_route_21a438_to_workspace_hub_signe_935143.rs");
}
