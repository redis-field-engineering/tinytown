/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Audit logging for townhall operations.
//!
//! This module provides structured audit logging for all state-changing operations
//! in the townhall API. Audit events include:
//! - Request ID for correlation
//! - Principal (authenticated identity)
//! - Action performed
//! - Target resources (agent/task IDs)
//! - Result (success/denied/error)
//!
//! # Security Notes
//! - Never logs raw tokens, API keys, or authorization headers
//! - All sensitive data is redacted before logging

use axum::{body::Body, extract::Request, http::Method, middleware::Next, response::Response};
use serde::Serialize;
use tracing::{info, warn};
use uuid::Uuid;

use super::auth::Principal;

/// Audit event for a state-changing operation.
#[derive(Debug, Serialize)]
pub struct AuditEvent {
    /// Unique request identifier for correlation
    pub request_id: String,
    /// Principal that made the request
    pub principal_id: String,
    /// Scopes the principal has
    pub scopes: Vec<String>,
    /// HTTP method
    pub method: String,
    /// Request path
    pub path: String,
    /// Result of the operation
    pub result: AuditResult,
}

/// Result of an audited operation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditResult {
    /// Operation succeeded
    Success,
    /// Operation was denied (auth failure)
    Denied,
    /// Operation failed with an error
    Error,
}

impl AuditEvent {
    /// Create a new audit event.
    pub fn new(
        request_id: impl Into<String>,
        principal: &Principal,
        method: &Method,
        path: impl Into<String>,
        result: AuditResult,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            principal_id: principal.id.clone(),
            scopes: principal.scopes.iter().map(|s| s.to_string()).collect(),
            method: method.to_string(),
            path: path.into(),
            result,
        }
    }

    /// Log this audit event.
    pub fn log(&self) {
        match self.result {
            AuditResult::Success => {
                info!(
                    target: "audit",
                    request_id = %self.request_id,
                    principal = %self.principal_id,
                    method = %self.method,
                    path = %self.path,
                    result = "success",
                    "audit: operation completed"
                );
            }
            AuditResult::Denied => {
                warn!(
                    target: "audit",
                    request_id = %self.request_id,
                    principal = %self.principal_id,
                    method = %self.method,
                    path = %self.path,
                    result = "denied",
                    "audit: operation denied"
                );
            }
            AuditResult::Error => {
                warn!(
                    target: "audit",
                    request_id = %self.request_id,
                    principal = %self.principal_id,
                    method = %self.method,
                    path = %self.path,
                    result = "error",
                    "audit: operation failed"
                );
            }
        }
    }
}

/// Middleware that logs audit events for mutating operations (POST, PUT, DELETE, PATCH).
pub async fn audit_middleware(request: Request<Body>, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let request_id = Uuid::new_v4().to_string();

    // Only audit mutating operations
    let is_mutating = matches!(
        method,
        Method::POST | Method::PUT | Method::DELETE | Method::PATCH
    );

    if !is_mutating {
        return next.run(request).await;
    }

    // Get principal from request extensions (set by auth middleware)
    let principal = request
        .extensions()
        .get::<Principal>()
        .cloned()
        .unwrap_or_else(|| Principal {
            id: "anonymous".to_string(),
            scopes: std::collections::HashSet::new(),
        });

    let response = next.run(request).await;

    // Determine result based on response status
    let result = if response.status().is_success() {
        AuditResult::Success
    } else if response.status().as_u16() == 401 || response.status().as_u16() == 403 {
        AuditResult::Denied
    } else {
        AuditResult::Error
    };

    let event = AuditEvent::new(request_id, &principal, &method, path, result);
    event.log();

    response
}
