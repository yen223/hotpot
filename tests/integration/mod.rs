pub mod cli_commands;
pub mod file_storage;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;
use std::fs;
use std::io::Write;

pub struct TestContext {
    pub temp_dir: TempDir,
    pub file_path: PathBuf,
}

impl TestContext {
    pub fn new() -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let file_path = temp_dir.path().join("test_accounts.json");
        
        Self {
            temp_dir,
            file_path,
        }
    }

    pub fn with_empty_file() -> Self {
        let ctx = Self::new();
        fs::write(&ctx.file_path, r#"{"accounts": []}"#).expect("Failed to create empty file");
        ctx
    }

    pub fn with_test_accounts() -> Self {
        let ctx = Self::new();
        let test_data = r#"{
  "accounts": [
    {
      "name": "github",
      "secret": "JBSWY3DPEHPK3PXP",
      "issuer": "",
      "algorithm": "SHA1",
      "digits": 6,
      "period": 30,
      "epoch": 0
    },
    {
      "name": "google",
      "secret": "HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ",
      "issuer": "",
      "algorithm": "SHA1", 
      "digits": 6,
      "period": 30,
      "epoch": 0
    }
  ]
}"#;
        fs::write(&ctx.file_path, test_data).expect("Failed to write test data");
        ctx
    }

    pub fn file_path(&self) -> &Path {
        &self.file_path
    }
}

pub fn run_hotpot_command(args: &[&str]) -> Output {
    let mut cmd = Command::new("cargo");
    cmd.arg("run");
    cmd.arg("--");
    cmd.args(args);
    
    cmd.output()
        .expect("Failed to execute hotpot command")
}

pub fn run_hotpot_with_input(args: &[&str], input: &str) -> Output {
    let mut cmd = Command::new("cargo");
    cmd.arg("run");
    cmd.arg("--");
    cmd.args(args);
    
    let mut child = cmd
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to spawn hotpot command");
    
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(input.as_bytes()).expect("Failed to write to stdin");
    }
    
    child.wait_with_output().expect("Failed to wait for command")
}

pub fn assert_totp_valid(output: &str) {
    // Extract the code from output like "Code for github: 123456"
    let code = if let Some(colon_pos) = output.rfind(':') {
        output[colon_pos + 1..].trim()
    } else {
        output.trim()
    };
    
    assert_eq!(code.len(), 6, "TOTP code should be 6 digits, got: '{}'", code);
    assert!(code.chars().all(|c| c.is_ascii_digit()), "TOTP code should only contain digits, got: '{}'", code);
}

pub fn file_contains_account(file_path: &Path, account_name: &str) -> bool {
    if let Ok(content) = fs::read_to_string(file_path) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(accounts) = json.get("accounts").and_then(|a| a.as_array()) {
                return accounts.iter().any(|acc| {
                    acc.get("name").and_then(|n| n.as_str()) == Some(account_name)
                });
            }
        }
    }
    false
}

pub fn get_account_count(file_path: &Path) -> usize {
    if let Ok(content) = fs::read_to_string(file_path) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(accounts) = json.get("accounts").and_then(|a| a.as_array()) {
                return accounts.len();
            }
        }
    }
    0
}