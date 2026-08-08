//! user-service library: authentication module for user registration, login,
//! logout, and session management.
//!
//! Modules are re-exported here so integration tests can access them via
//! `user_service::config::Config`, etc.

pub mod config;
pub mod middleware;
pub mod models;
pub mod routes;
pub mod services;
pub mod utils;
