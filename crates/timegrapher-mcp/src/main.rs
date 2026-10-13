//! `timegrapher-mcp`: the timegrapher as an MCP server over stdio, for chat
//! apps that cannot run a command line (docs/mcp.md). Every tool runs the
//! `timegrapher` program with `--json` and returns its document unchanged,
//! so this server and the command line give the same answers.

mod caveats;
mod cli;
mod guidance;
mod live;
mod progress;
mod summary;
mod takes;
mod tools;
mod views;

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

/// A result, built from its wire form, marked complete. The 2026-07-28
/// revision of MCP requires `resultType` on every result and rmcp leaves it
/// out of a result built from JSON without it; rmcp takes it off again for
/// chat apps on older revisions.
fn complete<T: DeserializeOwned>(mut v: Value) -> Result<T, ErrorData> {
    v["resultType"] = json!("complete");
    wire(v)
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
    complete(json!({ "content": [{ "type": "text", "text": text }], "isError": true }))
        .expect("a text result")
}

async fn call(
    name: &str,
    args: &Map<String, Value>,
    context: &RequestContext<RoleServer>,
) -> Result<CallToolResult, ErrorData> {
    let Some(tool) = tools::find(name) else {
        return Err(ErrorData::invalid_params(
            format!("no tool named {name}"),
            None,
        ));
    };
    let mut line = match tools::command_line(tool, args) {
        Ok(line) => line,
        Err(e) => return Ok(tool_error(e)),
    };
    // A recording the chat app retries with the same take_id is answered
    // from the take already made; two calls never share the microphone.
    let take = (tool.name == "record_position")
        .then(|| takes::key(args))
        .flatten();
    if let Some(r) = take.as_ref().and_then(takes::recorded) {
        return complete(r);
    }
    let _mic = if tools::listens(tool) {
        match takes::claim_mic() {
            Some(mic) => Some(mic),
            None => return Ok(tool_error(takes::BUSY.into())),
        }
    } else {
        None
    };
    // The Recording view's readings over time come from the same run.
    let recording_view = tool.name == "analyze_recording";
    let (extras, _windows) = views::analyze_extras();
    if recording_view {
        line.splice(1..1, extras.iter().cloned());
    }
    // Tests read a file in place of the microphone, which CI does not have.
    if tool.name == "record_position" {
        if let Some(f) = std::env::var_os("TIMEGRAPHER_MCP_TEST_INPUT") {
            line.insert(1, format!("--file={}", f.to_string_lossy()));
        }
    }
    // While it records, the command keeps its latest readings in a file
    // for the Live view and the progress messages, when the chat app can
    // show either; the file goes when the call ends.
    let feed = (tool.name == "record_position" && live::wanted(context)).then(live::Feed::start);
    if let Some(f) = &feed {
        line.insert(1, format!("--live={}", f.path().display()));
    }
    // A recording takes as long as it records: say how far it has got, and
    // stop it (the child goes with the future) when the chat app cancels.
    let progress = (tool.name == "record_position")
        .then(|| progress::report(context, args, feed.as_ref().map(|f| f.path().to_path_buf())))
        .flatten();
    let out = tokio::select! {
        out = cli::run(&line) => out,
        () = context.ct.cancelled() => Err("cancelled".to_string()),
    };
    if let Some(p) = progress {
        p.abort();
    }
    let out = match out {
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
    // What only the charts draw stays out of what the model reads.
    let mut doc = doc;
    let series_charts =
        (tool.name == "check_steadiness").then(|| views::take_series_charts(&mut doc));
    // The text blocks carry, first, a lead in plain words (the readings
    // with their standard errors, what limits them, notes), so a model reads
    // that before the numbers; then the same document as the structured
    // content, for chat apps that show the model only text, with any
    // warnings from stderr after it.
    let mut lead: Vec<String> = summary::lead(&doc, args).into_iter().collect();
    lead.extend(tools::notes(tool).into_iter().map(|n| format!("Note: {n}")));
    let mut text = serde_json::to_string(&doc).unwrap_or_default();
    if !out.stderr.is_empty() {
        text.push_str("\n\nThe command also said:\n");
        text.push_str(&out.stderr);
    }
    let mut content = Vec::new();
    if !lead.is_empty() {
        content.push(json!({ "type": "text", "text": lead.join("\n\n") }));
    }
    content.push(json!({ "type": "text", "text": text }));
    let mut result = json!({
        "content": content,
        "structuredContent": doc,
    });
    if recording_view {
        let recording = line.last().cloned().unwrap_or_default();
        let charts = views::analyze_charts(&doc, args, &extras, &recording).await;
        result["_meta"] = json!({ views::META_KEY: charts });
    }
    if let Some(charts) = series_charts {
        result["_meta"] = json!({ views::META_KEY: charts });
    }
    // What limits the readings, in the same words as the notes, for the view.
    let limits = caveats::limits(&doc);
    if !limits.is_empty() {
        if !result["_meta"].is_object() {
            result["_meta"] = json!({});
        }
        result["_meta"][caveats::META_KEY] = json!(limits);
    }
    if let Some(k) = take {
        takes::remember(k, &result);
    }
    complete(result)
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
        let list: Vec<Value> = tools::TOOLS
            .iter()
            .map(tools::definition)
            .chain([guidance::guide_definition(), live::definition()])
            .collect();
        complete(json!({ "tools": list }))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let args = request.arguments.unwrap_or_default();
        if request.name == guidance::GUIDE_TOOL {
            let page = args.get("page").and_then(Value::as_str);
            return Ok(complete::<CallToolResult>(guidance::guide(page))?.into());
        }
        if request.name == live::TOOL {
            return Ok(complete::<CallToolResult>(live::status())?.into());
        }
        Ok(call(&request.name, &args, &context).await?.into())
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
            .chain(std::iter::once(views::resource()))
            .collect();
        complete(json!({ "resources": list }))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        if request.uri == views::URI {
            let result: ReadResourceResult = complete(json!({ "contents": [views::contents()] }))?;
            return Ok(result.into());
        }
        let Some(d) = guidance::find(&request.uri) else {
            return Err(ErrorData::resource_not_found(
                format!("no resource {}", request.uri),
                None,
            ));
        };
        let result: ReadResourceResult = complete(json!({
            "contents": [{ "uri": d.uri, "mimeType": "text/markdown", "text": d.text }],
        }))?;
        Ok(result.into())
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        complete(json!({ "prompts": [{
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
        let result: GetPromptResult = complete(json!({
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
