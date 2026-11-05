use crate::tools::tool_definitions;
use serde_json::{Value, json};

pub const CORE: &[&str] = &[
    "doc_create",
    "file_attach",
    "open_document",
    "save_document",
    "inspect_document",
    "get_text",
    "select_text",
    "type_text",
    "execute",
    "list_commands",
    "batch",
    "new_document",
    "list_documents",
    "render_page",
];

pub fn definitions() -> Vec<Value> {
    let mut tools: Vec<Value> =
        tool_definitions().as_array().cloned().unwrap_or_default().into_iter().filter(|t| CORE.iter().any(|n| t["name"] == *n)).collect();
    tools.push(json!({"name":"tool_search", "description":"Find any WordCraft tool by query/category (name prefix), or return a full schema by exact name. Run it with tool_call.",
        "inputSchema":{"type":"object","additionalProperties":false,"properties":{
            "query":{"type":"string"}, "category":{"type":"string"}, "name":{"type":"string"}
        }}}));
    tools.push(json!({"name":"tool_call", "description":"Run any WordCraft tool by name and arguments. Returns the same content, including downloadable files, as a direct call. Cannot call meta tools.",
        "inputSchema":{"type":"object","additionalProperties":false,"required":["name"],"properties":{
            "name":{"type":"string"}, "arguments":{"type":"object"}
        }}}));
    tools
}

pub fn search(args: &Value) -> Result<Value, String> {
    let obj = args.as_object().ok_or("tool_search needs an object")?;
    for (key, value) in obj {
        if !["query", "category", "name"].contains(&key.as_str()) || !value.is_string() {
            return Err("tool_search accepts string query, category and name only".into());
        }
    }
    let tools = tool_definitions().as_array().cloned().unwrap_or_default();
    if let Some(name) = args.get("name").and_then(Value::as_str) {
        return tools.into_iter().find(|t| t["name"] == name).ok_or_else(|| format!("unknown tool {name}"));
    }
    let query = args.get("query").and_then(Value::as_str).unwrap_or_default().to_lowercase();
    let category = args.get("category").and_then(Value::as_str).unwrap_or_default().to_lowercase();
    Ok(Value::Array(
        tools
            .into_iter()
            .filter(|t| {
                t["name"].as_str().is_some_and(|n| n.starts_with(&category))
                    && query.split_whitespace().all(|q| t.to_string().to_lowercase().contains(q))
            })
            .map(|t| json!({"name":t["name"],"description":t["description"]}))
            .collect(),
    ))
}
