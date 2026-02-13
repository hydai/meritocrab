use axum::{
    extract::{FromRequest, Request},
    http::{StatusCode, header::HeaderMap},
    response::{IntoResponse, Response},
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

/// Webhook secret for HMAC verification
#[derive(Clone)]
pub struct WebhookSecret(String);

impl WebhookSecret {
    pub fn new(secret: String) -> Self {
        Self(secret)
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

/// Verified webhook payload extractor
///
/// This extractor validates the HMAC-SHA256 signature from GitHub webhooks.
/// It extracts the `X-Hub-Signature-256` header and validates it against the request body.
///
/// Usage:
/// ```ignore
/// async fn webhook_handler(
///     VerifiedWebhook(body): VerifiedWebhook,
/// ) -> impl IntoResponse {
///     // body is verified and can be parsed safely
/// }
/// ```
#[derive(Debug)]
pub struct VerifiedWebhook(pub Vec<u8>);

impl FromRequest<WebhookSecret> for VerifiedWebhook {
    type Rejection = WebhookError;

    async fn from_request(req: Request, state: &WebhookSecret) -> Result<Self, Self::Rejection> {
        let (parts, body) = req.into_parts();

        // Extract signature from header
        let signature = extract_signature(&parts.headers)?;

        // Read body bytes
        let body_bytes = axum::body::to_bytes(body, usize::MAX)
            .await
            .map_err(|e| WebhookError::BodyReadError(e.to_string()))?
            .to_vec();

        // Verify HMAC
        verify_signature(&body_bytes, &signature, state.expose())?;

        Ok(VerifiedWebhook(body_bytes))
    }
}

/// Extract signature from X-Hub-Signature-256 header
fn extract_signature(headers: &HeaderMap) -> Result<Vec<u8>, WebhookError> {
    let signature_header = headers
        .get("X-Hub-Signature-256")
        .ok_or_else(|| {
            WebhookError::MissingHeader("X-Hub-Signature-256 header not found".to_string())
        })?
        .to_str()
        .map_err(|e| WebhookError::InvalidSignature(format!("Invalid header encoding: {}", e)))?;

    // GitHub sends signature as "sha256=<hex>"
    let signature_hex = signature_header.strip_prefix("sha256=").ok_or_else(|| {
        WebhookError::InvalidSignature("Signature must start with 'sha256='".to_string())
    })?;

    // Decode hex to bytes
    hex::decode(signature_hex)
        .map_err(|e| WebhookError::InvalidSignature(format!("Invalid hex encoding: {}", e)))
}

/// Verify HMAC-SHA256 signature using constant-time comparison
fn verify_signature(body: &[u8], signature: &[u8], secret: &str) -> Result<(), WebhookError> {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|e| WebhookError::HmacError(format!("HMAC initialization failed: {}", e)))?;

    mac.update(body);
    let expected = mac.finalize().into_bytes();

    // Constant-time comparison to prevent timing attacks
    if expected.ct_eq(signature).into() {
        Ok(())
    } else {
        Err(WebhookError::VerificationFailed(
            "Signature mismatch".to_string(),
        ))
    }
}

/// Webhook verification error
#[derive(Debug)]
pub enum WebhookError {
    MissingHeader(String),
    InvalidSignature(String),
    HmacError(String),
    VerificationFailed(String),
    BodyReadError(String),
}

impl IntoResponse for WebhookError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            WebhookError::MissingHeader(msg) => (StatusCode::BAD_REQUEST, msg),
            WebhookError::InvalidSignature(msg) => (StatusCode::BAD_REQUEST, msg),
            WebhookError::HmacError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            WebhookError::VerificationFailed(msg) => (StatusCode::UNAUTHORIZED, msg),
            WebhookError::BodyReadError(msg) => (StatusCode::BAD_REQUEST, msg),
        };

        (status, message).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};

    fn compute_signature(body: &[u8], secret: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let result = mac.finalize();
        format!("sha256={}", hex::encode(result.into_bytes()))
    }

    #[tokio::test]
    async fn test_valid_signature() {
        let secret = WebhookSecret::new("test-secret".to_string());
        let body = b"test body";
        let signature = compute_signature(body, "test-secret");

        let req = Request::builder()
            .header("X-Hub-Signature-256", signature)
            .body(Body::from(body.to_vec()))
            .unwrap();

        let result = VerifiedWebhook::from_request(req, &secret).await;
        assert!(result.is_ok());
        let verified = result.unwrap();
        assert_eq!(verified.0, body);
    }

    #[tokio::test]
    async fn test_invalid_signature() {
        let secret = WebhookSecret::new("test-secret".to_string());
        let body = b"test body";
        let wrong_signature =
            "sha256=0000000000000000000000000000000000000000000000000000000000000000";

        let req = Request::builder()
            .header("X-Hub-Signature-256", wrong_signature)
            .body(Body::from(body.to_vec()))
            .unwrap();

        let result = VerifiedWebhook::from_request(req, &secret).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        matches!(err, WebhookError::VerificationFailed(_));
    }

    #[tokio::test]
    async fn test_missing_signature_header() {
        let secret = WebhookSecret::new("test-secret".to_string());
        let body = b"test body";

        let req = Request::builder().body(Body::from(body.to_vec())).unwrap();

        let result = VerifiedWebhook::from_request(req, &secret).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        matches!(err, WebhookError::MissingHeader(_));
    }

    #[tokio::test]
    async fn test_empty_body() {
        let secret = WebhookSecret::new("test-secret".to_string());
        let body = b"";
        let signature = compute_signature(body, "test-secret");

        let req = Request::builder()
            .header("X-Hub-Signature-256", signature)
            .body(Body::from(body.to_vec()))
            .unwrap();

        let result = VerifiedWebhook::from_request(req, &secret).await;
        assert!(result.is_ok());
        let verified = result.unwrap();
        assert_eq!(verified.0, body);
    }

    #[tokio::test]
    async fn test_invalid_signature_format() {
        let secret = WebhookSecret::new("test-secret".to_string());
        let body = b"test body";

        let req = Request::builder()
            .header("X-Hub-Signature-256", "not-a-valid-signature")
            .body(Body::from(body.to_vec()))
            .unwrap();

        let result = VerifiedWebhook::from_request(req, &secret).await;
        assert!(result.is_err());
        matches!(result.unwrap_err(), WebhookError::InvalidSignature(_));
    }

    #[tokio::test]
    async fn test_signature_without_prefix() {
        let secret = WebhookSecret::new("test-secret".to_string());
        let body = b"test body";

        let req = Request::builder()
            .header("X-Hub-Signature-256", "0123456789abcdef")
            .body(Body::from(body.to_vec()))
            .unwrap();

        let result = VerifiedWebhook::from_request(req, &secret).await;
        assert!(result.is_err());
        matches!(result.unwrap_err(), WebhookError::InvalidSignature(_));
    }
}
