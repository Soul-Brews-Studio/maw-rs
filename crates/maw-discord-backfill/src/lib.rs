#![allow(clippy::pedantic)]

pub mod api;
pub mod engine;
pub mod error;
pub mod output;
pub mod token;

#[cfg(feature = "index")]
pub mod index;

pub use error::{Error, Result};
