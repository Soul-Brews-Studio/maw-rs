//! XDG/legacy maw path resolver ported from maw-js `src/core/xdg.ts`,
//! `src/core/paths.ts`, and `src/cli/instance-preset.ts`.

mod config;
mod paths;
mod types;

pub use config::{
    deep_merge_config, discover_config_layers, load_merged_config, load_merged_config_in_dir,
};
pub use paths::{
    ensure_maw_core_paths, is_maw_xdg_enabled, is_valid_instance_name, maw_cache_dir,
    maw_cache_path, maw_config_dir, maw_config_path, maw_core_paths, maw_data_dir, maw_data_path,
    maw_runtime_home_dir, maw_state_dir, maw_state_path, resolve_home,
};
pub use types::{
    MawConfigLayerSource, MawConfigScope, MawCorePaths, MawXdgEnv, MergedMawConfig,
};

#[cfg(test)]
mod tests;
