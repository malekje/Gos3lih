//! Tauri backend — thin bridge that forwards IPC commands to the
//! high-privilege Gos3lih service via Named Pipe.

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const PIPE_NAME: &str = r"\\.\pipe\gos3lih-ipc";

/// Forward a JSON-RPC–style message to the backend pipe and return the response.
#[tauri::command]
pub async fn ipc_forward(method: String, params: Option<serde_json::Value>) -> Result<serde_json::Value, String> {
    let request = match params {
        Some(p) => serde_json::json!({ "method": method, "params": p }),
        None => serde_json::json!({ "method": method }),
    };

    let response = send_to_pipe(&serde_json::to_string(&request).map_err(|e| e.to_string())?)
        .await
        .map_err(|e| format!("IPC error: {e}"))?;

    let parsed: serde_json::Value = serde_json::from_str(&response).map_err(|e| e.to_string())?;

    if parsed.get("status").and_then(|s| s.as_str()) == Some("error") {
        let msg = parsed
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown error");
        return Err(msg.to_string());
    }

    Ok(parsed.get("data").cloned().unwrap_or(serde_json::Value::Null))
}

/// Connect to the named pipe, send a line, read a line back.
async fn send_to_pipe(request_json: &str) -> anyhow::Result<String> {
    use interprocess::local_socket::{
        tokio::prelude::*,
        GenericNamespaced,
    };

    let name = PIPE_NAME.to_ns_name::<GenericNamespaced>()?;
    let conn = interprocess::local_socket::tokio::Stream::connect(name).await?;

    let (reader, mut writer) = tokio::io::split(conn);

    let mut msg = request_json.to_string();
    msg.push('\n');
    writer.write_all(msg.as_bytes()).await?;
    writer.flush().await?;

    let mut lines = BufReader::new(reader).lines();
    let response = lines
        .next_line()
        .await?
        .ok_or_else(|| anyhow::anyhow!("Pipe closed without response"))?;

    Ok(response)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![ipc_forward])
        .run(tauri::generate_context!())
        .expect("error running Gos3lih UI");
}
