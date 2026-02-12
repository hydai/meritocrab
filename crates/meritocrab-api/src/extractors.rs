use crate::{error::ApiError, state::AppState};
use axum::{
    extract::{FromRequest, Request},
    http::header::HeaderMap,
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

/// Verified webhook payload extractor that works with AppState
///
/// This extractor validates the HMAC-SHA256 signature from GitHub webhooks.
/// It extracts the `X-Hub-Signature-256` header and validates it against the request body.
#[derive(Debug)]
pub struct VerifiedWebhookPayload(pub Vec<u8>);

impl FromRequest<AppState> for VerifiedWebhookPayload {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &AppState) -> Result<Self, Self::Rejection> {
        let (parts, body) = req.into_parts();

        // Extract signature from header
        let signature = extract_signature(&parts.headers)?;

        // Read body bytes
        let body_bytes = axum::body::to_bytes(body, usize::MAX)
            .await
            .map_err(|e| ApiError::Internal(format!("Failed to read request body: {}", e)))?
            .to_vec();

        // Verify HMAC using webhook secret from app state
        verify_signature(&body_bytes, &signature, state.webhook_secret.expose())?;

        Ok(VerifiedWebhookPayload(body_bytes))
    }
}

/// Extract signature from X-Hub-Signature-256 header
fn extract_signature(headers: &HeaderMap) -> Result<Vec<u8>, ApiError> {
    let signature_header = headers
        .get("X-Hub-Signature-256")
        .ok_or_else(|| {
            ApiError::InvalidSignature("X-Hub-Signature-256 header not found".to_string())
        })?
        .to_str()
        .map_err(|e| {
            ApiError::InvalidSignature(format!("Invalid header encoding: {}", e))
        })?;

    // GitHub sends signature as "sha256=<hex>"
    let signature_hex = signature_header
        .strip_prefix("sha256=")
        .ok_or_else(|| {
            ApiError::InvalidSignature("Signature must start with 'sha256='".to_string())
        })?;

    // Decode hex to bytes
    hex::decode(signature_hex).map_err(|e| {
        ApiError::InvalidSignature(format!("Invalid hex encoding: {}", e))
    })
}

/// Verify HMAC-SHA256 signature using constant-time comparison
fn verify_signature(body: &[u8], signature: &[u8], secret: &str) -> Result<(), ApiError> {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).map_err(|e| {
        ApiError::Internal(format!("HMAC initialization failed: {}", e))
    })?;

    mac.update(body);
    let expected = mac.finalize().into_bytes();

    // Constant-time comparison to prevent timing attacks
    if expected.ct_eq(signature).into() {
        Ok(())
    } else {
        Err(ApiError::InvalidSignature(
            "Signature mismatch".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compute_signature(body: &[u8], secret: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let result = mac.finalize();
        format!("sha256={}", hex::encode(result.into_bytes()))
    }

    #[test]
    fn test_extract_signature_valid() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Hub-Signature-256",
            "sha256=0123456789abcdef".parse().unwrap(),
        );

        let result = extract_signature(&headers);
        assert!(result.is_ok());
    }

    #[test]
    fn test_extract_signature_missing() {
        let headers = HeaderMap::new();
        let result = extract_signature(&headers);
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_signature_invalid_format() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Hub-Signature-256",
            "invalid-format".parse().unwrap(),
        );

        let result = extract_signature(&headers);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_signature_valid() {
        let body = b"test body";
        let secret = "test-secret";
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let signature = mac.finalize().into_bytes();

        let result = verify_signature(body, &signature, secret);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_signature_invalid() {
        let body = b"test body";
        let secret = "test-secret";
        let wrong_signature = [0u8; 32];

        let result = verify_signature(body, &wrong_signature, secret);
        assert!(result.is_err());
    }
}
