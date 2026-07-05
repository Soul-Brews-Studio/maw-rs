#[cfg(test)]
mod coverage_gap_tests {
    include!("classifier_trait_tests.rs");
    include!("router_failover_tests.rs");
    include!("transport_lifecycle_tests.rs");
    include!("http_feed_warning_tests.rs");
}
