/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Application services for Tinytown.
//!
//! This module provides the shared business logic used by both the `tt` CLI
//! and the `townhall` REST API daemon. By extracting orchestration operations
//! into reusable services, we ensure consistent behavior across all interfaces.
//!
//! ## Architecture
//!
//! ```text
//! ┌────────────┐     ┌──────────────┐
//! │   tt CLI   │     │  townhall    │
//! └─────┬──────┘     └──────┬───────┘
//!       │                   │
//!       ▼                   ▼
//! ┌─────────────────────────────────┐
//! │     Application Services        │
//! │  (agents, tasks, backlog, etc)  │
//! └─────────────────────────────────┘
//!       │
//!       ▼
//! ┌─────────────────────────────────┐
//! │   Town / Channel (Redis)        │
//! └─────────────────────────────────┘
//! ```

pub mod audit;
pub mod auth;
pub mod rate_limit;
pub mod server;
pub mod services;

pub use audit::{AuditEvent, AuditResult, audit_middleware};
pub use auth::{AuthError, AuthState, Principal, auth_middleware, generate_api_key, require_scope};
pub use rate_limit::{RateLimitConfig, RateLimiter, rate_limit_middleware};
pub use server::{AppState, ProblemDetails, create_router};
pub use services::*;
