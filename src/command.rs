use anyhow::{Result, Context};
use std::env;
use std::path::Path;
use std::process::Command;
use log::{debug, error};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct AIResponse {
    input: String,
    command: Option<String>,
    error: Option<String>,
}

pub fn execute_command(cmd: &str, clear_callback: &mut dyn FnMut()) -> Result<String> {
    debug!("Executing command: {}", cmd);
    if cmd.trim() == "clear" {
        debug!("Clear command detected, calling clear callback");
        clear_callback();
        return Ok("".to_string());
    }

    let output = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .env("TERM", "xterm-256color")
        .output()
        .context(format!("Failed to execute command: {}", cmd))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let result = if output.status.success() {
        debug!("Command succeeded: {}", stdout);
        stdout
    } else {
        error!("Command failed: {}", stderr);
        format!("{}\n{}", stdout, stderr)
    };
    Ok(result)
}

pub fn process_ai_command(input: &str) -> Result<String> {
    debug!("Processing AI command: {}", input);
    let output = Command::new("python3")
        .arg("/projects/terminalAI/ai_agent.py")
        .arg(input)
        .output()
        .context("Failed to call AI agent script")?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        error!("AI script failed: {}", stderr);
        return Err(anyhow::anyhow!("AI script error: {}", stderr));
    }

    let response: AIResponse = serde_json::from_str(&stdout)
        .context(format!("Failed to parse AI response: {}", stdout))?;

    if let Some(error) = response.error {
        error!("AI error: {}", error);
        return Err(anyhow::anyhow!("AI processing error: {}", error));
    }

    let command = response.command.unwrap_or_default();
    if command.is_empty() {
        error!("No command returned by AI for input: {}", input);
        return Err(anyhow::anyhow!("No valid command returned by AI"));
    }

    debug!("AI returned command: {}", command);
    Ok(command)
}

pub fn change_directory(dir: &str) -> Result<()> {
    debug!("Changing directory to: {}", dir);
    let path = if dir.is_empty() {
        env::var("HOME").context("HOME environment variable not set")?
    } else {
        let current_dir = env::current_dir()?;
        let resolved_path = if dir.starts_with('/') {
            Path::new(dir).to_path_buf()
        } else {
            current_dir.join(dir)
        };
        resolved_path
            .canonicalize()
            .context(format!("Directory {} does not exist", dir))?
            .to_str()
            .context("Invalid path encoding")?
            .to_string()
    };
    env::set_current_dir(&path).context(format!("Failed to change directory to {}", path))?;
    debug!("Directory changed to: {}", path);
    Ok(())
}