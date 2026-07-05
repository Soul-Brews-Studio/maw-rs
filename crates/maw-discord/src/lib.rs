#![allow(
    clippy::pedantic,
    clippy::module_name_repetitions,
    clippy::too_many_lines
)]

pub mod gateway;

use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env, fs,
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

mod access_core;
mod access_read;
mod access_write;
mod bind;
mod command;
mod core;
mod pair_route;
mod rest_commands;
mod rest_helpers;
mod serve;
mod status;
mod status_emit;
mod tokens;
mod util;
mod validation;

pub use self::command::{run_discord_command, run_discord_command_with};
pub use self::core::{
    DiscordEnv, DiscordHttpResponse, DiscordOutput, DiscordRest, ReqwestDiscordRest, TokenEntry,
};
pub use self::util::is_numeric_snowflake;

use self::{
    access_core::*, access_read::*, access_write::*, bind::*, core::*, pair_route::*,
    rest_commands::*,
    rest_helpers::*, serve::*, status::*, status_emit::*, tokens::*, util::*, validation::*,
};

#[cfg(test)]
mod tests;
