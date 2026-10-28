//! Object-safe ports. Concrete HTTP, CLI, MQTT and MCP adapters belong in infrastructure.
pub mod capabilities;
mod context;
mod error;
pub mod events;
pub mod lifecycle;
pub mod messages;
pub mod tools;

pub use context::OperationContext;
pub use error::{PortError, RetryAdvice};

/// Boxed futures allow Arc<dyn Trait> composition without requiring async-trait macros.
pub type PortFuture<'a, T> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, PortError>> + Send + 'a>>;
pub mod audit;
pub mod delivery;
pub mod identity;
pub mod profiles;
pub mod routing;

pub mod recovery;

pub mod checkpoints;

pub mod content;

pub mod invocations;

pub mod publications;

pub mod workspaces;

pub mod mcp_content;

pub mod connection_routing;

pub mod connection_actions;
pub mod connection_data;

pub mod recent_messages;
