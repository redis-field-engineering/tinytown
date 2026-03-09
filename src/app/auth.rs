/*
 * Copyright (c) 2024-Present, Jeremy Plichta
 * Licensed under the MIT License
 */

//! Authentication and authorization middleware for townhall.
//!
//! This module provides Tower layers for:
//! - API key authentication (Bearer token or X-API-Key header)
//! - Scope-based authorization
//! - Principal extraction for audit logging
//!
//! # Security Notes
//! - API keys are stored as Argon2id hashes, never in plaintext
//! - Authorization header values are never logged
//! - All auth errors use constant-shape responses to avoid timing attacks

use std::collections::HashSet;
use std::sync::Arc;

use argon2::{Argon2, PasswordHash, PasswordVerifier};
use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::config::{AuthConfig, AuthMode, Scope};

/// Authenticated principal information extracted from request.
#[derive(Debug, Clone)]
pub struct Principal {
    /// Unique identifier for the principal (e.g., API key ID, JWT sub)
    pub id: String,
    /// Scopes granted to this principal
    pub scopes: HashSet<Scope>,
}

impl Principal {
    /// Create a new principal with full admin access (for local/no-auth mode).
    #[must_use]
    pub fn local_admin() -> Self {
        Self {
            id: "local".to_string(),
            scopes: HashSet::from([
                Scope::TownRead,
                Scope::TownWrite,
                Scope::AgentManage,
                Scope::Admin,
            ]),
        }
    }

    /// Create a new principal with specific scopes.
    /// If scopes is empty, grants all scopes (admin).
    #[must_use]
    pub fn with_scopes(id: impl Into<String>, scopes: &[Scope]) -> Self {
        let scopes = if scopes.is_empty() {
            // Default to full access if no scopes specified
            HashSet::from([
                Scope::TownRead,
                Scope::TownWrite,
                Scope::AgentManage,
                Scope::Admin,
            ])
        } else {
            scopes.iter().copied().collect()
        };
        Self {
            id: id.into(),
            scopes,
        }
    }

    /// Check if principal has a specific scope.
    #[must_use]
    pub fn has_scope(&self, scope: Scope) -> bool {
        self.scopes.contains(&scope) || self.scopes.contains(&Scope::Admin)
    }
}

/// Shared authentication state.
#[derive(Clone)]
pub struct AuthState {
    pub config: Arc<AuthConfig>,
}

/// Authentication error response.
#[derive(Debug)]
pub struct AuthError {
    status: StatusCode,
    message: &'static str,
}

impl AuthError {
    pub const UNAUTHORIZED: Self = Self {
        status: StatusCode::UNAUTHORIZED,
        message: "Authentication required",
    };

    pub const FORBIDDEN: Self = Self {
        status: StatusCode::FORBIDDEN,
        message: "Insufficient permissions",
    };

    pub const INVALID_CREDENTIALS: Self = Self {
        status: StatusCode::UNAUTHORIZED,
        message: "Invalid credentials",
    };
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "error": self.message,
            "status": self.status.as_u16()
        });
        (self.status, axum::Json(body)).into_response()
    }
}

/// Extract API key from request headers.
/// Supports both `Authorization: Bearer <key>` and `X-API-Key: <key>` headers.
fn extract_api_key(headers: &HeaderMap) -> Option<String> {
    // Try Authorization header first (Bearer token)
    if let Some(auth) = headers.get("authorization")
        && let Ok(auth_str) = auth.to_str()
        && let Some(key) = auth_str.strip_prefix("Bearer ")
    {
        return Some(key.trim().to_string());
    }
    // Fall back to X-API-Key header
    if let Some(key) = headers.get("x-api-key")
        && let Ok(key_str) = key.to_str()
    {
        return Some(key_str.trim().to_string());
    }
    None
}

/// Verify API key against stored hash.
fn verify_api_key(key: &str, hash: &str) -> bool {
    let parsed_hash = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(key.as_bytes(), &parsed_hash)
        .is_ok()
}

/// Authentication middleware that extracts and validates credentials.
pub async fn auth_middleware(
    State(auth_state): State<AuthState>,
    mut request: Request<Body>,
    next: Next,
) -> Result<Response, AuthError> {
    let config = &auth_state.config;

    let principal = match config.mode {
        AuthMode::None => Principal::local_admin(),
        AuthMode::ApiKey => {
            let key = extract_api_key(request.headers()).ok_or(AuthError::UNAUTHORIZED)?;
            let hash = config
                .api_key_hash
                .as_ref()
                .ok_or(AuthError::UNAUTHORIZED)?;
            if !verify_api_key(&key, hash) {
                return Err(AuthError::INVALID_CREDENTIALS);
            }
            // Use configured scopes for API key auth (defaults to admin if empty)
            Principal::with_scopes("api_key", &config.api_key_scopes)
        }
        AuthMode::Oidc => return Err(AuthError::UNAUTHORIZED), // TODO: Implement OIDC
    };

    request.extensions_mut().insert(principal);
    Ok(next.run(request).await)
}

/// Authorization middleware that checks if principal has required scope.
pub async fn require_scope(
    scope: Scope,
    request: Request<Body>,
    next: Next,
) -> Result<Response, AuthError> {
    let principal = request
        .extensions()
        .get::<Principal>()
        .ok_or(AuthError::UNAUTHORIZED)?;

    if !principal.has_scope(scope) {
        return Err(AuthError::FORBIDDEN);
    }

    Ok(next.run(request).await)
}

/// Route scope constants for easy import.
/// These directly re-export the Scope variants for cleaner route configuration.
pub mod route_scopes {
    pub use crate::config::Scope::{
        Admin as ADMIN_OPS, AgentManage as AGENT_MGMT, TownRead as READ_OPS, TownWrite as WRITE_OPS,
    };
}

/// Generate an API key and its Argon2id hash.
/// Returns (raw_key, hash) - only store the hash!
#[must_use]
pub fn generate_api_key() -> (String, String) {
    use argon2::{PasswordHasher, password_hash::SaltString};

    // Generate a random key using two UUIDs concatenated (64 hex chars)
    let raw_key = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );

    // Generate salt from UUID bytes (provides 122 bits of randomness)
    let salt_uuid = uuid::Uuid::new_v4();
    let salt = SaltString::encode_b64(salt_uuid.as_bytes()).expect("valid salt");
    let hash = Argon2::default()
        .hash_password(raw_key.as_bytes(), &salt)
        .expect("failed to hash password")
        .to_string();

    (raw_key, hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_api_key_bearer() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer test-key-123".parse().unwrap());
        assert_eq!(extract_api_key(&headers), Some("test-key-123".to_string()));
    }

    #[test]
    fn test_extract_api_key_x_api_key() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", "test-key-456".parse().unwrap());
        assert_eq!(extract_api_key(&headers), Some("test-key-456".to_string()));
    }

    #[test]
    fn test_extract_api_key_bearer_priority() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer bearer-key".parse().unwrap());
        headers.insert("x-api-key", "x-api-key".parse().unwrap());
        // Bearer takes priority
        assert_eq!(extract_api_key(&headers), Some("bearer-key".to_string()));
    }

    #[test]
    fn test_extract_api_key_none() {
        let headers = HeaderMap::new();
        assert_eq!(extract_api_key(&headers), None);
    }

    #[test]
    fn test_generate_and_verify_api_key() {
        let (raw_key, hash) = generate_api_key();
        assert!(verify_api_key(&raw_key, &hash));
        assert!(!verify_api_key("wrong-key", &hash));
    }

    #[test]
    fn test_principal_has_scope() {
        let admin = Principal::local_admin();
        assert!(admin.has_scope(Scope::TownRead));
        assert!(admin.has_scope(Scope::TownWrite));
        assert!(admin.has_scope(Scope::AgentManage));
        assert!(admin.has_scope(Scope::Admin));
    }

    #[test]
    fn test_principal_admin_grants_all() {
        let mut scopes = HashSet::new();
        scopes.insert(Scope::Admin);
        let admin = Principal {
            id: "admin".to_string(),
            scopes,
        };
        // Admin scope grants all others
        assert!(admin.has_scope(Scope::TownRead));
        assert!(admin.has_scope(Scope::TownWrite));
        assert!(admin.has_scope(Scope::AgentManage));
    }
}
