use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;

const DEFAULT_URL: &str = "http://localhost:8000";

#[derive(Parser, Debug)]
#[command(name = "ask", about = "CLI tool for querying a local vLLM instance")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Base URL of the vLLM server
    #[arg(short, long)]
    pub url: Option<String>,

    /// Model name (fetched from server if not provided)
    #[arg(short, long)]
    pub model: Option<String>,

    /// Message role
    #[arg(short, long, default_value = "user")]
    pub role: String,

    /// The prompt text. If omitted, reads from stdin.
    pub text: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Set the default server URL
    SetUrl {
        /// The URL to save as the default
        url: String,
    },
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Config {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"))
        .join("ask")
        .join("config.json")
}

pub fn load_config_from(path: &PathBuf) -> Config {
    match fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
        Err(_) => Config::default(),
    }
}

pub fn save_config_to(path: &PathBuf, config: &Config) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {e}"))?;
    }
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialise config: {e}"))?;
    fs::write(path, json).map_err(|e| format!("Failed to write config: {e}"))?;
    Ok(())
}

pub fn resolve_url(cli_url: Option<String>, config: &Config) -> String {
    cli_url
        .or_else(|| config.url.clone())
        .unwrap_or_else(|| DEFAULT_URL.to_string())
}

#[derive(Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Deserialize, Debug)]
pub struct ModelsResponse {
    pub data: Vec<ModelEntry>,
}

#[derive(Deserialize, Debug)]
pub struct ModelEntry {
    pub id: String,
}

#[derive(Deserialize, Debug)]
pub struct ChatResponse {
    pub choices: Vec<Choice>,
}

#[derive(Deserialize, Debug)]
pub struct Choice {
    pub message: MessageContent,
}

#[derive(Deserialize, Debug)]
pub struct MessageContent {
    pub content: String,
}

pub fn resolve_text(text: Option<String>) -> Result<String, String> {
    match text {
        Some(t) if !t.is_empty() => Ok(t),
        _ => {
            if atty::is(atty::Stream::Stdin) {
                return Err(
                    "No prompt provided. Pass text as an argument or pipe via stdin.".into(),
                );
            }
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| format!("Failed to read stdin: {e}"))?;
            let trimmed = buf.trim().to_string();
            if trimmed.is_empty() {
                Err("Empty input from stdin.".into())
            } else {
                Ok(trimmed)
            }
        }
    }
}

pub async fn fetch_model(client: &reqwest::Client, base_url: &str) -> Result<String, String> {
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to connect to {base_url}: {e}"))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response from {url}: {e}"))?;

    if !status.is_success() {
        return Err(format!(
            "Server at {base_url} returned {status}. Is this a vLLM-compatible server?\nResponse: {body}"
        ));
    }

    let resp: ModelsResponse = serde_json::from_str(&body).map_err(|_| {
        let preview = if body.len() > 200 {
            format!("{}...", &body[..200])
        } else {
            body.clone()
        };
        format!(
            "Server at {base_url} didn't return a valid models response. Is this a vLLM-compatible server?\nGot: {preview}"
        )
    })?;

    resp.data
        .first()
        .map(|m| m.id.clone())
        .ok_or_else(|| "No models available on the server.".into())
}

pub async fn send_chat(
    client: &reqwest::Client,
    base_url: &str,
    request: &ChatRequest,
) -> Result<String, String> {
    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
    let resp: ChatResponse = client
        .post(&url)
        .json(request)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse chat response: {e}"))?;

    resp.choices
        .first()
        .map(|c| c.message.content.clone())
        .ok_or_else(|| "No response from model.".into())
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let cfg_path = config_path();

    if let Some(Commands::SetUrl { url }) = cli.command {
        let config = Config {
            url: Some(url.clone()),
        };
        match save_config_to(&cfg_path, &config) {
            Ok(_) => {
                println!("Default URL set to: {url}");
                return;
            }
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
    }

    let config = load_config_from(&cfg_path);
    let url = resolve_url(cli.url, &config);

    let content = match resolve_text(cli.text) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    let client = reqwest::Client::new();

    let model = match cli.model {
        Some(m) => m,
        None => match fetch_model(&client, &url).await {
            Ok(m) => m,
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        },
    };

    let request = ChatRequest {
        model,
        messages: vec![Message {
            role: cli.role,
            content,
        }],
    };

    match send_chat(&client, &url, &request).await {
        Ok(reply) => println!("{reply}"),
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_config_path() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ask-test-{}", std::process::id()));
        dir.join("config.json")
    }

    #[test]
    fn test_resolve_text_with_value() {
        let result = resolve_text(Some("hello".into()));
        assert_eq!(result.unwrap(), "hello");
    }

    #[test]
    fn test_resolve_text_empty_string() {
        let result = resolve_text(Some("".into()));
        // In test, stdin is not a tty so it reads stdin (which is empty)
        assert!(result.is_err());
    }

    #[test]
    fn test_chat_request_serialisation() {
        let req = ChatRequest {
            model: "test-model".into(),
            messages: vec![Message {
                role: "user".into(),
                content: "hello".into(),
            }],
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["model"], "test-model");
        assert_eq!(json["messages"][0]["role"], "user");
        assert_eq!(json["messages"][0]["content"], "hello");
    }

    #[test]
    fn test_models_response_deserialisation() {
        let json = r#"{"data": [{"id": "my-model", "object": "model"}]}"#;
        let resp: ModelsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.data[0].id, "my-model");
    }

    #[test]
    fn test_chat_response_deserialisation() {
        let json = r#"{
            "choices": [{
                "message": {"role": "assistant", "content": "42"},
                "index": 0,
                "finish_reason": "stop"
            }]
        }"#;
        let resp: ChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.choices[0].message.content, "42");
    }

    #[test]
    fn test_save_and_load_config() {
        let path = temp_config_path();
        let config = Config {
            url: Some("http://my-server:9000".into()),
        };
        save_config_to(&path, &config).unwrap();

        let loaded = load_config_from(&path);
        assert_eq!(loaded.url, Some("http://my-server:9000".into()));

        // cleanup
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn test_load_config_missing_file() {
        let path = PathBuf::from("/tmp/ask-nonexistent/config.json");
        let config = load_config_from(&path);
        assert_eq!(config.url, None);
    }

    #[test]
    fn test_load_config_invalid_json() {
        let path = temp_config_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "not json").unwrap();

        let config = load_config_from(&path);
        assert_eq!(config.url, None);

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn test_resolve_url_cli_overrides_config() {
        let config = Config {
            url: Some("http://config-server:8000".into()),
        };
        let result = resolve_url(Some("http://cli-server:8000".into()), &config);
        assert_eq!(result, "http://cli-server:8000");
    }

    #[test]
    fn test_resolve_url_falls_back_to_config() {
        let config = Config {
            url: Some("http://config-server:8000".into()),
        };
        let result = resolve_url(None, &config);
        assert_eq!(result, "http://config-server:8000");
    }

    #[test]
    fn test_resolve_url_falls_back_to_default() {
        let config = Config::default();
        let result = resolve_url(None, &config);
        assert_eq!(result, DEFAULT_URL);
    }

    #[tokio::test]
    async fn test_fetch_model_from_server() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/v1/models")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"data": [{"id": "llama-3", "object": "model"}]}"#)
            .create_async()
            .await;

        let client = reqwest::Client::new();
        let model = fetch_model(&client, &server.url()).await.unwrap();
        assert_eq!(model, "llama-3");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_fetch_model_empty() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/v1/models")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"data": []}"#)
            .create_async()
            .await;

        let client = reqwest::Client::new();
        let result = fetch_model(&client, &server.url()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_send_chat() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/v1/chat/completions")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"choices": [{"message": {"role": "assistant", "content": "the answer is 42"}, "index": 0, "finish_reason": "stop"}]}"#)
            .create_async()
            .await;

        let client = reqwest::Client::new();
        let req = ChatRequest {
            model: "test".into(),
            messages: vec![Message {
                role: "user".into(),
                content: "what is the meaning of life?".into(),
            }],
        };
        let reply = send_chat(&client, &server.url(), &req).await.unwrap();
        assert_eq!(reply, "the answer is 42");
        mock.assert_async().await;
    }
}
