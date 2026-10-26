//! RFC 7662 introspection against an explicitly configured authorization server.
use super::security::OAuthConfig;
use crate::{
    application::ports::{
        PortError, PortFuture,
        identity::{Authenticator, Credentials},
    },
    domain::identity::Identity,
};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Semaphore;
use uuid::Uuid;

pub struct OAuthAuthenticator {
    pub config: OAuthConfig,
    workspace: Uuid,
    credential: String,
    client: reqwest::Client,
    slots: Semaphore,
}
#[derive(Deserialize)]
struct Introspection {
    active: bool,
    iss: Option<String>,
    aud: Option<Value>,
    sub: Option<String>,
    client_id: Option<String>,
    exp: Option<u64>,
    nbf: Option<u64>,
    iat: Option<u64>,
    scope: Option<String>,
    token_type: Option<String>,
}
impl OAuthAuthenticator {
    pub fn new(
        config: OAuthConfig,
        workspace: Uuid,
        credential: String,
    ) -> Result<Self, PortError> {
        if credential.is_empty()
            || credential.len() > 4096
            || credential.bytes().any(|b| b < 33 || b == 127)
        {
            return Err(PortError::InvalidInput {
                code: "invalid_introspection_secret",
            });
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(3))
            .build()
            .map_err(|_| PortError::Unavailable)?;
        Ok(Self {
            config,
            workspace,
            credential,
            client,
            slots: Semaphore::new(8),
        })
    }
    pub fn metadata(&self) -> Value {
        serde_json::json!({"resource":self.config.resource,"authorization_servers":[self.config.issuer],"scopes_supported":[self.config.required_scope],"bearer_methods_supported":["header"],"resource_name":"Synquay MCP"})
    }
    pub fn metadata_url(&self) -> String {
        let mut url = reqwest::Url::parse(&self.config.resource).expect("validated resource URL");
        url.set_path("/.well-known/oauth-protected-resource/mcp");
        url.to_string()
    }
    pub fn challenge(&self, error: &str) -> String {
        format!(
            "Bearer resource_metadata=\"{}\", scope=\"{}\", error=\"{}\"",
            self.metadata_url(),
            self.config.required_scope,
            error
        )
    }
}
impl Authenticator for OAuthAuthenticator {
    fn authenticate(&self, credentials: Credentials) -> PortFuture<'_, Identity> {
        Box::pin(async move {
            let token = credentials.bearer_token;
            if token.is_empty()
                || token.len() > 8192
                || !token.bytes().all(|b| (33..=126).contains(&b))
            {
                return Err(PortError::PermissionDenied);
            }
            let _permit = self
                .slots
                .try_acquire()
                .map_err(|_| PortError::RateLimited {
                    retry_after: Duration::from_secs(1),
                })?;
            let response = self
                .client
                .post(&self.config.introspection_endpoint)
                .basic_auth(
                    url::form_urlencoded::byte_serialize(
                        self.config.introspection_client_id.as_bytes(),
                    )
                    .collect::<String>(),
                    Some(
                        url::form_urlencoded::byte_serialize(self.credential.as_bytes())
                            .collect::<String>(),
                    ),
                )
                .form(&[
                    ("token", token.as_str()),
                    ("token_type_hint", "access_token"),
                ])
                .send()
                .await
                .map_err(|_| PortError::Unavailable)?;
            if !response.status().is_success()
                || response.content_length().is_some_and(|n| n > 16 * 1024)
            {
                return Err(PortError::Unavailable);
            }
            let mut bytes = Vec::new();
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|_| PortError::Unavailable)?;
                if bytes.len() + chunk.len() > 16 * 1024 {
                    return Err(PortError::Unavailable);
                }
                bytes.extend_from_slice(&chunk);
            }
            let data: Introspection =
                serde_json::from_slice(&bytes).map_err(|_| PortError::Unavailable)?;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| PortError::Unavailable)?
                .as_secs();
            let audience = match &data.aud {
                Some(Value::String(s)) => s == &self.config.resource,
                Some(Value::Array(items)) => {
                    items.iter().all(Value::is_string)
                        && items
                            .iter()
                            .any(|v| v.as_str() == Some(&self.config.resource))
                }
                _ => false,
            };
            if !data.active
                || data.iss.as_deref() != Some(&self.config.issuer)
                || !audience
                || data.exp.is_none_or(|e| e <= now)
                || data.nbf.is_some_and(|n| n > now)
                || data.iat.is_some_and(|i| i > now)
                || data
                    .token_type
                    .as_deref()
                    .is_some_and(|t| !t.eq_ignore_ascii_case("Bearer"))
            {
                return Err(PortError::PermissionDenied);
            }
            let grant = self
                .config
                .grants
                .iter()
                .find(|g| {
                    Some(g.subject.as_str()) == data.sub.as_deref()
                        && Some(g.oauth_client_id.as_str()) == data.client_id.as_deref()
                })
                .ok_or(PortError::PermissionDenied)?;
            let scope = data.scope.unwrap_or_default();
            if scope.len() > 2048 || !scope.bytes().all(|b| (32..=126).contains(&b)) {
                return Err(PortError::PermissionDenied);
            }
            let scopes: BTreeSet<String> =
                scope.split_ascii_whitespace().map(str::to_owned).collect();
            let expires_at = UNIX_EPOCH
                .checked_add(Duration::from_secs(
                    data.exp.ok_or(PortError::PermissionDenied)?,
                ))
                .ok_or(PortError::PermissionDenied)?;
            Ok(Identity {
                mcp_endpoint: None,
                workspace_id: self.workspace,
                principal_id: grant.principal_id,
                client_id: Some(grant.client_id),
                scopes,
                expires_at: Some(expires_at),
            })
        })
    }
}
