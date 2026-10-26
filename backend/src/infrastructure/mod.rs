//! Runtime adapters: telemetry, persistence, content, connectors and pipeline workers.
pub mod capabilities;
pub mod shutdown;
pub mod telemetry;

pub mod connectors;
pub mod pipeline;
pub mod storage;

pub mod content;

pub mod tool_runtime;

pub mod mcp_content;
