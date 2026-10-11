//! `timegrapher-mcp`: the timegrapher as an MCP server over stdio, for chat
//! apps that cannot run a command line (docs/mcp.md). Every tool runs the
//! `timegrapher` program with `--json` and returns its document unchanged,
//! so this server and the command line give the same answers.

mod cli;
mod guidance;
mod tools;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, GetPromptRequestParams,
    GetPromptResponse, GetPromptResult, InitializeResult, ListPromptsResult, ListResourcesResult,
    ListToolsResult, PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResponse,
    ReadResourceResult,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde::de::DeserializeOwned;
use serde_json::{json, Map, Value};

struct Server;

/// The MCP types are built from their wire form, which is what the spec
/// documents and what a reader of this file can check against it.
fn wire<T: DeserializeOwned>(v: Value) -> Result<T, ErrorData> {
    serde_json::from_value(v).map_err(|e| ErrorData::internal_error(e.to_string(), None))
}

fn info() -> Value {
    json!({
        "protocolVersion": rmcp::model::ProtocolVersion::default(),
        "capabilities": { "tools": {}, "resources": {}, "prompts": {} },
        "serverInfo": {
            "name": "timegrapher",
            "title": "Timegrapher",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "instructions": guidance::INSTRUCTIONS,
    })
}

/// A failed call the agent can read and act on (MCP's `isError` result,
/// rather than a protocol error the chat app would hide).
fn tool_error(text: String) -> CallToolResult {
    wire(json!({ "content": [{ "type": "text", "text": text }], "isError": true }))
        .expect("a text result")
}

async fn call(name: &str, args: &Map<String, Value>) -> Result<CallToolResult, ErrorData> {
    let Some(tool) = tools::find(name) else {
        return Err(ErrorData::invalid_params(
            format!("no tool named {name}"),
            None,
        ));
    };
    let line = match tools::command_line(tool, args) {
        Ok(line) => line,
        Err(e) => return Ok(tool_error(e)),
    };
    let out = match cli::run(&line).await {
        Ok(out) => out,
        Err(e) => return Ok(tool_error(e)),
    };
    let Some(doc) = out.json else {
        let code = out
            .exit_code
            .map_or("no exit code".to_string(), |c| format!("exit code {c}"));
        let said = if out.stderr.is_empty() {
            "nothing".to_string()
        } else {
            out.stderr
        };
        return Ok(tool_error(format!(
            "timegrapher {} failed ({code}): {said}",
            line[0]
        )));
    };
    // The text block carries the same document for chat apps that show the
    // model only text; warnings from stderr ride along after it.
    let mut text = serde_json::to_string(&doc).unwrap_or_default();
    if !out.stderr.is_empty() {
        text.push_str("\n\nThe command also said:\n");
        text.push_str(&out.stderr);
    }
    wire(json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": doc,
    }))
}

impl ServerHandler for Server {
    fn get_info(&self) -> InitializeResult {
        wire(info()).expect("server info")
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let list: Vec<Value> = tools::TOOLS.iter().map(tools::definition).collect();
        wire(json!({ "tools": list }))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let args = request.arguments.unwrap_or_default();
        Ok(call(&request.name, &args).await?.into())
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let list: Vec<Value> = guidance::DOCS
            .iter()
            .map(|d| {
                json!({
                    "uri": d.uri,
                    "name": d.name,
                    "title": d.title,
                    "description": d.description,
                    "mimeType": "text/markdown",
                    "size": d.text.len(),
                })
            })
            .collect();
        wire(json!({ "resources": list }))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        let Some(d) = guidance::find(&request.uri) else {
            return Err(ErrorData::resource_not_found(
                format!("no resource {}", request.uri),
                None,
            ));
        };
        let result: ReadResourceResult = wire(json!({
            "contents": [{ "uri": d.uri, "mimeType": "text/markdown", "text": d.text }],
        }))?;
        Ok(result.into())
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        wire(json!({ "prompts": [{
            "name": guidance::PROMPT_NAME,
            "title": guidance::PROMPT_TITLE,
            "description": guidance::PROMPT_DESCRIPTION,
            "arguments": [{
                "name": "watch",
                "description": "The watch or its calibre, if you know it.",
                "required": false,
            }],
        }]}))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, ErrorData> {
        if request.name != guidance::PROMPT_NAME {
            return Err(ErrorData::invalid_params(
                format!("no prompt named {}", request.name),
                None,
            ));
        }
        let watch = request
            .arguments
            .as_ref()
            .and_then(|a| a.get("watch"))
            .and_then(|w| w.as_str());
        let result: GetPromptResult = wire(json!({
            "description": guidance::PROMPT_DESCRIPTION,
            "messages": [{
                "role": "user",
                "content": { "type": "text", "text": guidance::prompt_text(watch) },
            }],
        }))?;
        Ok(result.into())
    }
}

fn main() -> std::process::ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("timegrapher-mcp {}", env!("CARGO_PKG_VERSION"));
        return std::process::ExitCode::SUCCESS;
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let served = runtime.block_on(async {
        let service = Server.serve(rmcp::transport::stdio()).await?;
        service.waiting().await?;
        Ok::<_, Box<dyn std::error::Error>>(())
    });
    match served {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            // stdout is the protocol; anything for people goes to stderr.
            eprintln!("timegrapher-mcp: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
