use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use base32::{Alphabet, decode};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Parser)]
#[command(name = "authy-replacement")]
#[command(about = "A simple CLI for TOTP-based 2FA", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Add a new account with secret
    Add {
        /// Account name (e.g., email or service identifier)
        name: String,
        /// Base32 encoded secret
        secret: String,
    },
    /// Generate code for an account
    Code {
        /// Account name to generate code for
        name: String,
    },
    /// List all configured accounts
    List,
}

#[derive(Serialize, Deserialize)]
struct Account {
    name: String,
    secret: String,
}

fn main() {
    let cli = Cli::parse();
    let data_dir = directories::ProjectDirs::from("com", "example", "authy-replacement")
        .expect("Cannot determine data directory")
        .data_dir()
        .to_path_buf();

    fs::create_dir_all(&data_dir).expect("Failed to create data directory");
    let config_file = data_dir.join("accounts.json");

    match &cli.command {
        Commands::Add { name, secret } => {
            let mut accounts = load_accounts(&config_file);
            accounts.push(Account {
                name: name.clone(),
                secret: secret.clone(),
            });
            save_accounts(&config_file, &accounts);
            println!("Added account: {}", name);
        }
        Commands::Code { name } => {
            let accounts = load_accounts(&config_file);
            if let Some(account) = accounts.iter().find(|a| &a.name == name) {
                let secret_bytes = decode(Alphabet::RFC4648 { padding: false }, &account.secret)
                    .expect("Failed to decode base32 secret");
                let code = {
                    let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                    let counter = duration / 30;
                    let mut mac = Hmac::<Sha1>::new_from_slice(&secret_bytes).expect("HMAC can take key");
                    mac.update(&counter.to_be_bytes());
                    let result = mac.finalize().into_bytes();
                    let offset = (result[19] & 0xf) as usize;
                    let binary = ((u32::from(result[offset]) & 0x7f) << 24)
                        | ((u32::from(result[offset + 1]) & 0xff) << 16)
                        | ((u32::from(result[offset + 2]) & 0xff) << 8)
                        | (u32::from(result[offset + 3]) & 0xff);
                    binary % 1_000_000
                };
                println!("Code for {}: {:06}", name, code);
            } else {
                eprintln!("Account not found: {}", name);
            }
        }
        Commands::List => {
            let accounts = load_accounts(&config_file);
            for account in accounts {
                println!("{}", account.name);
            }
        }
    }
}

fn load_accounts(path: &PathBuf) -> Vec<Account> {
    if let Ok(data) = fs::read_to_string(path) {
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        Vec::new()
    }
}

fn save_accounts(path: &PathBuf, accounts: &Vec<Account>) {
    let data = serde_json::to_string_pretty(accounts).expect("Failed to serialize accounts");
    fs::write(path, data).expect("Failed to write accounts file");
}
