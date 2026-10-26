//! Per-endpoint authenticators. Secrets remain environment references in persisted records.
use super::{
    ToolRuntime, config,
    oauth::OAuthAuthenticator,
    security::{OAuthConfig, OAuthGrant},
};
use crate::{
    application::ports::{
        PortError,
        identity::{Authenticator, Credentials},
    },
    domain::{
        identity::Identity,
        mcp_server::{McpServer, ServerAuth},
    },
};
use base64::{Engine, engine::general_purpose::STANDARD};
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use subtle::ConstantTimeEq;
use uuid::Uuid;

pub enum PreparedAuth {
    Inherited,
    None,
    Basic {
        username: String,
        password: String,
    },
    Jwt {
        key: DecodingKey,
        validation: Box<Validation>,
    },
    OAuth(Box<OAuthAuthenticator>),
}
fn invalid() -> PortError {
    PortError::InvalidInput {
        code: "invalid_server_auth",
    }
}
fn text(value: &str) -> bool {
    !value.is_empty() && value.len() <= 2048 && !value.chars().any(char::is_control)
}
fn same(a: &str, b: &str) -> bool {
    let a = ring::digest::digest(&ring::digest::SHA256, a.as_bytes());
    let b = ring::digest::digest(&ring::digest::SHA256, b.as_bytes());
    bool::from(a.as_ref().ct_eq(b.as_ref()))
}
fn url(value: &str, loopback: bool) -> Result<reqwest::Url, PortError> {
    let u = reqwest::Url::parse(value).map_err(|_| invalid())?;
    if value.len() > 2048
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
        || u.host_str().is_none()
        || !(u.scheme() == "https"
            || (loopback
                && u.scheme() == "http"
                && matches!(u.host_str(), Some("127.0.0.1" | "[::1]"))))
    {
        return Err(invalid());
    }
    Ok(u)
}
impl ToolRuntime {
    pub fn prepare_endpoint_auth(
        &self,
        id: Uuid,
        auth: &ServerAuth,
        path: &str,
    ) -> Result<PreparedAuth, PortError> {
        Ok(match auth {
            ServerAuth::Inherited => PreparedAuth::Inherited,
            ServerAuth::None => PreparedAuth::None,
            ServerAuth::Basic {
                username,
                password_env,
            } => {
                if !text(username) || username.len() > 256 || username.contains(':') {
                    return Err(invalid());
                }
                PreparedAuth::Basic {
                    username: username.clone(),
                    password: config::secret_value(password_env, 12)?,
                }
            }
            ServerAuth::Jwt {
                issuer,
                audience,
                subject,
                public_key_pem,
            } => {
                if !text(issuer)
                    || !text(audience)
                    || !text(subject)
                    || public_key_pem.len() > 8192
                    || public_key_pem.contains("PRIVATE")
                {
                    return Err(invalid());
                }
                let key =
                    DecodingKey::from_rsa_pem(public_key_pem.as_bytes()).map_err(|_| invalid())?;
                let mut validation = Validation::new(Algorithm::RS256);
                validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
                validation.set_issuer(&[issuer]);
                validation.set_audience(&[audience]);
                validation.sub = Some(subject.clone());
                validation.leeway = 0;
                validation.validate_nbf = true;
                PreparedAuth::Jwt {
                    key,
                    validation: Box::new(validation),
                }
            }
            ServerAuth::OAuth {
                issuer,
                resource,
                introspection_endpoint,
                introspection_client_id,
                introspection_secret_env,
                required_scope,
                subject,
                oauth_client_id,
                allow_loopback_http,
            } => {
                for v in [issuer, resource, introspection_endpoint] {
                    url(v, *allow_loopback_http)?;
                }
                if url(resource, *allow_loopback_http)?.path() != path
                    || !text(subject)
                    || !text(oauth_client_id)
                    || !text(introspection_client_id)
                    || required_scope.is_empty()
                    || required_scope.len() > 128
                    || !required_scope.bytes().all(|b| {
                        b == 0x21 || (0x23..=0x5b).contains(&b) || (0x5d..=0x7e).contains(&b)
                    })
                {
                    return Err(invalid());
                }
                let config = OAuthConfig {
                    issuer: issuer.clone(),
                    resource: resource.clone(),
                    introspection_endpoint: introspection_endpoint.clone(),
                    introspection_client_id: introspection_client_id.clone(),
                    introspection_secret_env: introspection_secret_env.clone(),
                    required_scope: required_scope.clone(),
                    allow_loopback_http: *allow_loopback_http,
                    grants: vec![OAuthGrant {
                        agent_jobs: false,
                        resources: vec![],
                        prompts: vec![],
                        subject: subject.clone(),
                        oauth_client_id: oauth_client_id.clone(),
                        principal_id: self.config.principal_id,
                        client_id: id,
                        tools: vec![],
                    }],
                };
                PreparedAuth::OAuth(Box::new(OAuthAuthenticator::new(
                    config,
                    self.config.workspace_id,
                    config::secret(introspection_secret_env)?,
                )?))
            }
        })
    }
    fn prepared(&self, server: &McpServer) -> Result<Arc<PreparedAuth>, PortError> {
        let mut cache = self
            .endpoint_auth_cache
            .lock()
            .map_err(|_| PortError::Unavailable)?;
        if let Some((revision, auth)) = cache.get(&server.id)
            && *revision == server.revision
        {
            return Ok(auth.clone());
        }
        let auth = Arc::new(self.prepare_endpoint_auth(
            server.id,
            &server.auth,
            &server.path(self.config.profile_id),
        )?);
        cache.insert(server.id, (server.revision, auth.clone()));
        Ok(auth)
    }
    pub async fn authenticate_endpoint(
        &self,
        server: &McpServer,
        header: Option<&str>,
    ) -> Result<Identity, PortError> {
        let auth = self.prepared(server)?;
        let bearer = || {
            header
                .and_then(|s| s.split_once(' '))
                .filter(|(scheme, token)| {
                    scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()
                })
                .map(|(_, token)| token)
                .ok_or(PortError::PermissionDenied)
        };
        let mut identity = Identity {
            mcp_endpoint: Some((server.id, server.revision)),
            workspace_id: self.config.workspace_id,
            principal_id: self.config.principal_id,
            client_id: Some(server.id),
            scopes: Default::default(),
            expires_at: None,
        };
        match auth.as_ref() {
            PreparedAuth::Inherited => {
                identity = self.authenticate_identity(bearer()?).await?;
            }
            PreparedAuth::None => {}
            PreparedAuth::Basic { username, password } => {
                let encoded = header
                    .and_then(|s| s.split_once(' '))
                    .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("basic"))
                    .map(|(_, v)| v)
                    .ok_or(PortError::PermissionDenied)?;
                let bytes = STANDARD
                    .decode(encoded)
                    .map_err(|_| PortError::PermissionDenied)?;
                let decoded =
                    std::str::from_utf8(&bytes).map_err(|_| PortError::PermissionDenied)?;
                let (user, pass) = decoded.split_once(':').ok_or(PortError::PermissionDenied)?;
                if !(same(user, username) & same(pass, password)) {
                    return Err(PortError::PermissionDenied);
                }
            }
            PreparedAuth::Jwt { key, validation } => {
                let claims = jsonwebtoken::decode::<Value>(bearer()?, key, validation)
                    .map_err(|_| PortError::PermissionDenied)?
                    .claims;
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| PortError::Unavailable)?
                    .as_secs();
                if claims["exp"].as_u64().is_none_or(|exp| exp <= now)
                    || claims
                        .get("iat")
                        .is_some_and(|v| v.as_u64().is_none_or(|n| n > now))
                {
                    return Err(PortError::PermissionDenied);
                }
                identity.expires_at = Some(
                    UNIX_EPOCH
                        .checked_add(Duration::from_secs(
                            claims["exp"].as_u64().ok_or(PortError::PermissionDenied)?,
                        ))
                        .ok_or(PortError::PermissionDenied)?,
                );
            }
            PreparedAuth::OAuth(o) => {
                identity = o
                    .authenticate(Credentials {
                        bearer_token: bearer()?.into(),
                    })
                    .await?;
            }
        }
        identity.mcp_endpoint = Some((server.id, server.revision));
        Ok(identity)
    }
    pub fn endpoint_metadata(&self, server: &McpServer) -> Option<Value> {
        match &server.auth {
            ServerAuth::Inherited => self.oauth.as_ref().map(|o| o.metadata()),
            ServerAuth::OAuth {
                issuer,
                resource,
                required_scope,
                ..
            } => Some(
                json!({"resource":resource,"authorization_servers":[issuer],"scopes_supported":[required_scope],"bearer_methods_supported":["header"]}),
            ),
            _ => None,
        }
    }
    pub fn endpoint_challenge(&self, server: &McpServer) -> String {
        if matches!(server.auth, ServerAuth::Basic { .. }) {
            return "Basic realm=\"synquay\", charset=\"UTF-8\"".into();
        }
        if let Some(metadata) = self.endpoint_metadata(server)
            && let Some(resource) = metadata["resource"].as_str()
            && let Ok(mut url) = reqwest::Url::parse(resource)
        {
            let path = format!("/.well-known/oauth-protected-resource{}", url.path());
            url.set_path(&path);
            return format!("Bearer resource_metadata=\"{url}\", error=\"invalid_token\"");
        }
        "Bearer realm=\"synquay\"".into()
    }
}
