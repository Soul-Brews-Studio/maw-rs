//! Serve-daemon module aggregator.
//!
//! Pattern-setter for the remaining serve-* fan-out PRs:
//! 1. Add one `serve_core/modules/<name>.rs` file.
//! 2. In that file expose `<name>_lifecycle_module() -> ServecoreLifecycleModule`,
//!    `<name>_mount(router) -> Router<S>`, and `<name>_registration() -> ServecoreModuleRegistration<S>`.
//!    `views` is the one approved special case: its mount is no-op and core owns the fallback.
//! 3. Add one alphabetically sorted line to `servecore_module_registry()`.
//! 4. If the module introduces a protected route, extend `maw_auth::is_protected()` in the same PR.
//! 5. Never mount after `servecore_apply_pipeline`; all module routers must pass through default-deny.

pub mod agent_routes;
pub mod debug_routes;
pub mod federation_routes;
pub mod god_mode_ui;
pub mod identity_routes;
pub mod pairing;
pub mod static_views;
pub mod thread_routes;
pub mod trigger_mutation_routes;
pub mod trigger_routes;
pub mod websocket_routes;
pub mod worktree_routes;
use super::{ServecoreLifecycle, ServecoreLifecycleModule};
use axum::Router;

pub struct ServecoreModuleRegistration<S>
where
    S: Clone + Send + Sync + 'static,
{
    pub lifecycle: ServecoreLifecycleModule,
    pub mount: fn(Router<S>) -> Router<S>,
}

pub fn servecore_mount_modules<S>(router: Router<S>, api_routers: &[String]) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let registrations = servecore_module_registry();
    let lifecycle = ServecoreLifecycle::servecore_from_profile(
        registrations
            .iter()
            .map(|registration| registration.lifecycle.clone())
            .collect(),
        api_routers,
    );
    let enabled = lifecycle.servecore_enabled_modules();
    registrations
        .into_iter()
        .filter(|registration| enabled.contains(&registration.lifecycle.name))
        .fold(router, |router, registration| (registration.mount)(router))
}

fn servecore_module_registry<S>() -> Vec<ServecoreModuleRegistration<S>>
where
    S: Clone + Send + Sync + 'static,
{
    vec![
        agent_routes::agents_registration(),
        debug_routes::debug_registration(),
        federation_routes::federation_registration(),
        god_mode_ui::godui_registration(),
        identity_routes::identity_registration(),
        pairing::pair_registration(),
        thread_routes::threadstore_registration(),
        trigger_routes::triggers_registration(),
        trigger_mutation_routes::triggersmutate_registration(),
        static_views::views_registration(),
        worktree_routes::worktrees_registration(),
        websocket_routes::ws_registration(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;

    #[test]
    fn servecore_module_aggregator_uses_lifecycle_and_whitelist() {
        let all: Router = servecore_mount_modules(Router::new(), &[]);
        let whitelisted: Router = servecore_mount_modules(Router::new(), &["agents".to_owned()]);
        let disabled: Router = servecore_mount_modules(Router::new(), &["debug".to_owned()]);
        let _ = (all, whitelisted, disabled);
    }

    #[test]
    fn servecore_module_registry_remains_name_sorted_for_parallel_fanout() {
        let names = servecore_module_registry::<()>()
            .into_iter()
            .map(|module| module.lifecycle.name)
            .collect::<Vec<_>>();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }
}
