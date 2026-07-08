use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MawCorePaths {
    pub runtime_home: PathBuf,
    pub config_dir: PathBuf,
    pub fleet_dir: PathBuf,
    pub config_file: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MawConfigScope {
    User,
    Project,
    Legacy,
}

impl MawConfigScope {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Project => "project",
            Self::Legacy => "legacy",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MawConfigLayerSource {
    pub path: PathBuf,
    pub weight: u32,
    pub is_local: bool,
    pub scope: MawConfigScope,
    pub scope_rank: u32,
    pub depth: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MergedMawConfig {
    pub config: Value,
    pub sources: Vec<MawConfigLayerSource>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MawXdgEnv {
    home_dir: PathBuf,
    vars: BTreeMap<String, String>,
}

impl MawXdgEnv {
    #[must_use]
    pub fn new(home_dir: impl Into<PathBuf>) -> Self {
        Self {
            home_dir: home_dir.into(),
            vars: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with_vars(
        home_dir: impl Into<PathBuf>,
        vars: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        let mut env = Self::new(home_dir);
        for (key, value) in vars {
            env.vars.insert(key.into(), value.into());
        }
        env
    }

    #[must_use]
    pub fn var(&self, name: &str) -> Option<&str> {
        self.vars.get(name).map(String::as_str)
    }

    #[must_use]
    pub fn home_dir(&self) -> &Path {
        &self.home_dir
    }
}
