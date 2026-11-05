use rmcp::{model::*, service::RequestContext, ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use tingskriver_core::{
    mcp::Reader,
    mcp_ipc::{self, Action},
};

#[derive(Clone)]
struct Server {
    dir: PathBuf,
}

fn tools() -> Vec<Tool> {
    let string = json!({"type":"string","minLength":1});
    [
        ("list_meetings", "List meetings allowed by the current sharing policy.", json!({"fra":string,"til":string,"søg":string}), vec![]),
        ("get_meeting", "Read selected parts of a shared meeting. Empty dele means all allowed parts.", json!({"id":string,"dele":{"type":"array","items":{"type":"string","enum":["referat","noter","transskript"]}}}), vec!["id"]),
        ("search", "Search local FTS5 content with excerpts and recording timestamps.", json!({"tekst":string}), vec!["tekst"]),
        ("list_tasks", "Read Markdown checklist tasks from meeting summaries.", json!({"åbne":{"type":"boolean"}}), vec![]),
        ("recent_dictations", "Read recent dictation text, application and time. Requires All access.", json!({"n":{"type":"integer","minimum":0,"maximum":500}}), vec![]),
        ("start_recording", "Start recording in the running app. Requires All access.", json!({"titel":string}), vec![]),
        ("stop_recording", "Stop the running recording. Requires All access.", json!({}), vec![]),
        ("regenerate_summary", "Regenerate a meeting summary using the app's configured provider and template.", json!({"id":string,"skabelon":string}), vec!["id","skabelon"]),
        ("handoff", "Send a shared meeting to an existing configured delivery target. This approves delivery of the current meeting content; the app's delivery privacy policy still applies.", json!({"id":string,"mål":string}), vec!["id","mål"]),
        ("add_note", "Append a Markdown note in the running app.", json!({"id":string,"tekst":string}), vec!["id","tekst"]),
    ].into_iter().map(|(name, description, properties, required)| {
        let schema = json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
        Tool::new(name, description, schema.as_object().unwrap().clone())
    }).collect()
}

fn text<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("Udfyld {key}"))
}

impl Server {
    async fn run_tool(&self, name: &str, args: Map<String, Value>) -> Result<Value, String> {
        let tool = tools()
            .into_iter()
            .find(|t| t.name == name)
            .ok_or("Ukendt værktøj")?;
        let properties = tool
            .input_schema
            .get("properties")
            .and_then(Value::as_object)
            .unwrap();
        if args.keys().any(|k| !properties.contains_key(k)) {
            return Err("Ukendt argument".into());
        }
        for (key, value) in &args {
            let expected = properties[key]["type"].as_str().unwrap_or("");
            if !match expected {
                "string" => value.is_string(),
                "integer" => value.as_u64().is_some_and(|n| n <= 500),
                "boolean" => value.is_boolean(),
                "array" => value.is_array(),
                _ => false,
            } {
                return Err(format!("Ugyldigt argument: {key}"));
            }
        }
        let reader = Reader::open(&self.dir.join(tingskriver_core::paths::DATABASE_FILE)).await?;
        let optional = |key: &str| args.get(key).and_then(Value::as_str);
        match name {
            "list_meetings" => Ok(json!(
                reader
                    .list_meetings(optional("fra"), optional("til"), optional("søg"))
                    .await?
            )),
            "get_meeting" => {
                let parts: Vec<String> =
                    serde_json::from_value(args.get("dele").cloned().unwrap_or(json!([])))
                        .map_err(|_| "Ugyldige mødele")?;
                reader.get_meeting(text(&args, "id")?, &parts).await
            }
            "search" => Ok(json!(reader.search(text(&args, "tekst")?).await?)),
            "list_tasks" => Ok(json!(
                reader
                    .list_tasks(args.get("åbne").and_then(Value::as_bool).unwrap_or(true))
                    .await?
            )),
            "recent_dictations" => Ok(json!(
                reader
                    .recent_dictations(
                        &self.dir.join("dictation.sqlite"),
                        args.get("n").and_then(Value::as_u64).unwrap_or(20) as u32
                    )
                    .await?
            )),
            _ => {
                let access = reader.access().await?;
                if matches!(
                    access,
                    tingskriver_core::mcp::Access::Off | tingskriver_core::mcp::Access::Metadata
                ) {
                    return Err("Handlinger er ikke tilladt på dette adgangsniveau.".into());
                }
                let action = match name {
                    "start_recording" => Action::StartRecording {
                        title: optional("titel").map(str::to_owned),
                    },
                    "stop_recording" => Action::StopRecording,
                    "regenerate_summary" => Action::RegenerateSummary {
                        id: text(&args, "id")?.into(),
                        template: text(&args, "skabelon")?.into(),
                    },
                    "handoff" => Action::Handoff {
                        id: text(&args, "id")?.into(),
                        target: text(&args, "mål")?.into(),
                    },
                    "add_note" => Action::AddNote {
                        id: text(&args, "id")?.into(),
                        text: text(&args, "tekst")?.into(),
                    },
                    _ => return Err("Ukendt værktøj".into()),
                };
                mcp_ipc::call(&self.dir, &action).await
            }
        }
    }
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }
    fn get_tool(&self, name: &str) -> Option<Tool> {
        tools().into_iter().find(|t| t.name == name)
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: tools(),
            ..Default::default()
        })
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let args = request.arguments.unwrap_or_default();
        let meeting_id = if let Some(id) = args.get("id").and_then(Value::as_str) {
            match Reader::open(&self.dir.join(tingskriver_core::paths::DATABASE_FILE)).await {
                Ok(reader) => reader.existing_id(id).await,
                Err(_) => None,
            }
        } else {
            None
        };
        let outcome = self.run_tool(&request.name, args).await;
        if tingskriver_core::mcp_audit::record(
            &self.dir,
            &request.name,
            meeting_id.as_deref(),
            outcome.is_ok(),
        )
        .await
        .is_err()
        {
            // Do not turn a completed action into a retryable failure.
            eprintln!("MCP-kaldsloggen kunne ikke gemmes.");
        }
        let result = match outcome {
            Ok(value) => CallToolResult::success(vec![ContentBlock::text(value.to_string())]),
            Err(message) => CallToolResult::error(vec![ContentBlock::text(message)]),
        };
        Ok(result.into())
    }
}

#[tokio::main]
async fn main() {
    let result = async {
        let mut args = std::env::args_os().skip(1);
        let dir = match args.next() {
            Some(flag) if flag == "--data-dir" => args
                .next()
                .map(PathBuf::from)
                .ok_or("--data-dir kræver en mappe")?,
            Some(_) => return Err("Brug tingskriver-mcp [--data-dir MAPPE]".to_string()),
            None => {
                tingskriver_core::paths::app_data_dir().ok_or("Datamappen kunne ikke findes")?
            }
        };
        if args.next().is_some() {
            return Err("For mange argumenter".into());
        }
        let server = Server { dir }
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|e| e.to_string())?;
        server.waiting().await.map_err(|e| e.to_string())?;
        Ok::<_, String>(())
    }
    .await;
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
