//! Usage: SYNQUAY_MCP_URL=http://127.0.0.1:8080/mcp SYNQUAY_MCP_TOKEN=... cargo run -p synquay-server --example mcp_client
//! Append: echo__r1 '{"value":7}' <stable-invocation-uuid> to invoke.
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ClientConfig},
    transport::streamable_http_client::{
        StreamableHttpClientTransport, StreamableHttpClientTransportConfig,
    },
};
use serde_json::json;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("SYNQUAY_MCP_URL")?;
    let token = std::env::var("SYNQUAY_MCP_TOKEN")?;
    let client = ClientConfig::default()
        .serve(StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(url).auth_header(token),
        ))
        .await?;
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        println!(
            "{}",
            serde_json::to_string_pretty(&client.list_all_tools().await?)?
        );
    } else {
        if args.len() != 3 {
            return Err("provide tool name, JSON object and a stable invocation UUID".into());
        }
        let id = uuid::Uuid::parse_str(&args[2])?;
        let mut request = CallToolRequestParams::new(args[0].clone())
            .with_arguments(serde_json::from_str(&args[1])?);
        request.meta = Some(serde_json::from_value(json!({"synquay/invocation-id":id}))?);
        println!(
            "{}",
            serde_json::to_string_pretty(&client.call_tool(request).await?)?
        );
    }
    client.cancel().await?;
    Ok(())
}
