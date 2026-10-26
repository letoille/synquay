use std::{error::Error, future::IntoFuture, path::Path, sync::Arc};
use synquay_server::{
    AppState,
    config::Config,
    infrastructure::{
        pipeline::{Pipeline, config::PipelineConfig},
        shutdown, telemetry,
    },
};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let config = Config::from_env()?;
    telemetry::init(config.log_format, config.log_filter)?;
    let pipeline = match std::env::var("SYNQUAY_PIPELINE_CONFIG") {
        Ok(path) => {
            let config = PipelineConfig::load(Path::new(&path))?;
            let (webhook, destination) = config.secrets()?;
            Some(Arc::new(
                Pipeline::build(config, webhook, destination).await?,
            ))
        }
        Err(std::env::VarError::NotPresent) => None,
        Err(_) => return Err("SYNQUAY_PIPELINE_CONFIG must contain valid Unicode".into()),
    };
    let security = match std::env::var("SYNQUAY_TOOLS_SECURITY_CONFIG") {
        Ok(path) => synquay_server::infrastructure::tool_runtime::security::SecurityConfig::load(
            Path::new(&path),
        )?,
        Err(std::env::VarError::NotPresent) => Default::default(),
        Err(_) => return Err("SYNQUAY_TOOLS_SECURITY_CONFIG must contain valid Unicode".into()),
    };
    let tools = match std::env::var("SYNQUAY_TOOLS_CONFIG") {
        Ok(path) => Some(
            synquay_server::infrastructure::tool_runtime::ToolRuntime::build_secured(
                synquay_server::infrastructure::tool_runtime::config::ToolsConfig::load(
                    Path::new(&path),
                )?,
                security,
            )
            .await?,
        ),
        Err(std::env::VarError::NotPresent) => None,
        Err(_) => return Err("SYNQUAY_TOOLS_CONFIG must contain valid Unicode".into()),
    };
    if let Some(p) = &pipeline
        && p.agent.is_some()
    {
        let runtime = tools
            .as_ref()
            .filter(|r| r.agent.is_some())
            .ok_or("Agent pipeline requires MCP agent_jobs=true")?;
        if std::fs::canonicalize(p.database_path())?
            != std::fs::canonicalize(&runtime.config.database)?
        {
            return Err("Agent pipeline and MCP must share the same database".into());
        }
    }
    let web = std::env::var_os("SYNQUAY_WEB_DIR").map(std::path::PathBuf::from);
    if let Some(path) = &web
        && !path.join("index.html").is_file()
    {
        return Err("SYNQUAY_WEB_DIR must contain the built index.html".into());
    }
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    let stop = CancellationToken::new();
    let mut worker = pipeline.clone().map(|p| tokio::spawn(p.run(stop.clone())));
    let signal_stop = stop.clone();
    let signal = tokio::spawn(async move {
        shutdown::signal().await;
        signal_stop.cancel();
    });
    tracing::info!(event="server.started", service="synquay-server",version=env!("CARGO_PKG_VERSION"),address=%listener.local_addr()?,pipeline_enabled=pipeline.is_some(),mcp_enabled=tools.is_some(),"Synquay server started");
    let state = AppState {
        pipeline,
        tools,
        ..Default::default()
    };
    let mut app = synquay_server::interfaces::http::router(state);
    if let Some(path) = web {
        app = synquay_server::interfaces::http::web::with_console(app, &path);
    }
    let server = axum::serve(listener, app).with_graceful_shutdown(stop.clone().cancelled_owned());
    let mut server = Box::pin(server.into_future());
    let server_result = if let Some(task) = worker.as_mut() {
        tokio::select! {
            result=&mut server=>result.map_err(|e|Box::new(e) as Box<dyn Error+Send+Sync>),
            result=task=>{
                let expected = stop.is_cancelled() && result.is_ok();
                stop.cancel();
                worker=None;
                let drained = server.await.map_err(|e|Box::new(e) as Box<dyn Error+Send+Sync>);
                if expected { drained } else { Err("pipeline worker terminated unexpectedly".into()) }
            }
        }
    } else {
        server
            .await
            .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
    };
    stop.cancel();
    if let Some(task) = worker {
        task.await
            .map_err(|_| "pipeline worker terminated unexpectedly")?;
    }
    signal.abort();
    server_result
}
