#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
include!("federation_auth/request_auth_contract.rs");
include!("federation_auth/pair_consent_plans.rs");
include!("federation_auth/expiry_helper_edges.rs");
