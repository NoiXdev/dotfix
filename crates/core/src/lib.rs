//! Core logic for dotfix. Contains no side effects: all interaction with
//! Homebrew, the filesystem and git goes through the traits in [`ports`].

pub mod about;
pub mod adopt;
pub mod agent;
pub mod apply;
pub mod config;
pub mod diffview;
pub mod doctor;
pub mod drift;
pub mod engine;
pub mod error;
pub mod init;
pub mod paths;
pub mod ports;
pub mod render;
pub mod secrets;
pub mod sets;
pub mod settings;
pub mod state;
pub mod status_line;

pub use error::{Error, Result};
