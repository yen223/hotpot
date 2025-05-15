use base32::{Alphabet, decode};
use clap::{Parser, Subcommand};
use hmac::{Hmac, Mac};
use keyring::Entry;
use rpassword::prompt_password;
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use std::error::Error;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

const SERVICE_NAME: &str = "hotpot";
const STORAGE_KEY: &str = "_hotpot_storage";

#[derive(Debug)]
pub struct AppError {
    message: String,
}

impl AppError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for AppError {}

impl From<keyring::Error> for AppError {
    fn from(err: keyring::Error) -> Self {
        Self::new(format!("Keyring error: {}", err))
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        Self::new(format!("Serialization error: {}", err))
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        Self::new(format!("IO error: {}", err))
    }
}

#[derive(Parser)]
#[command(name = "hotpot")]
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
    },
    /// Generate code for an account
    Code {
        /// Account name to generate code for
        name: String,
    },
    /// List all configured accounts
    List,
    /// Delete an account
    Delete {
        /// Account name to delete
        name: String,
    },
    /// Export account as QR code
    #[command(arg_required_else_help = true)]
    ExportQr {
        /// Account name to export
        #[arg(long)]
        name: String,
    },
}

#[derive(Serialize, Deserialize, Default)]
struct Storage {
    accounts: Vec<Account>,
}

#[derive(Serialize, Deserialize, Clone)]
struct Account {
    name: String,
    secret: String,
}

fn get_storage() -> Result<Storage, AppError> {
    let entry = Entry::new(SERVICE_NAME, STORAGE_KEY).map_err(AppError::from)?;

    match entry.get_password() {
        Ok(data) => Ok(serde_json::from_str(&data)?),
        Err(keyring::Error::NoEntry) => Ok(Storage::default()),
        Err(e) => Err(AppError::from(e)),
    }
}

fn save_storage(storage: &Storage) -> Result<(), AppError> {
    let data = serde_json::to_string(storage)?;
    Entry::new(SERVICE_NAME, STORAGE_KEY)?
        .set_password(&data)
        .map_err(AppError::from)
}

fn save_account(name: &str, secret: &str) -> Result<(), AppError> {
    let mut storage = get_storage()?;
    if storage.accounts.iter().any(|a| a.name == name) {
        return Err(AppError::new(format!("Account '{}' already exists", name)));
    }
    storage.accounts.push(Account {
        name: name.to_string(),
        secret: secret.to_string(),
    });
    storage.accounts.sort_by(|a, b| a.name.cmp(&b.name));
    save_storage(&storage)
}

fn get_account(name: &str) -> Result<Account, AppError> {
    let storage = get_storage()?;
    storage
        .accounts
        .iter()
        .find(|a| a.name == name)
        .cloned()
        .ok_or_else(|| AppError::new(format!("Account '{}' not found", name)))
}

fn delete_account(name: &str) -> Result<(), AppError> {
    let mut storage = get_storage()?;
    let initial_len = storage.accounts.len();
    storage.accounts.retain(|a| a.name != name);
    if storage.accounts.len() == initial_len {
        return Err(AppError::new(format!("Account '{}' not found", name)));
    }
    save_storage(&storage)
}

fn generate_totp(secret: &str) -> Result<u32, AppError> {
    let secret_bytes = match decode(Alphabet::RFC4648 { padding: false }, secret) {
        Some(bytes) => bytes,
        None => {
            return Err(AppError::new("Bytes could not be decoded"));
        }
    };

    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time is before Unix epoch");
    let counter = duration.as_secs() / 30;

    let mut mac =
        Hmac::<Sha1>::new_from_slice(&secret_bytes).expect("HMAC can take key of any size");
    mac.update(&counter.to_be_bytes());
    let result = mac.finalize().into_bytes();

    let offset = (result[19] & 0xf) as usize;
    let binary = ((u32::from(result[offset]) & 0x7f) << 24)
        | ((u32::from(result[offset + 1]) & 0xff) << 16)
        | ((u32::from(result[offset + 2]) & 0xff) << 8)
        | (u32::from(result[offset + 3]) & 0xff);

    Ok(binary % 1_000_000)
}

fn handle_error(err: AppError) {
    eprintln!("Error: {}", err);
    if let Some(source) = err.source() {
        eprintln!("Caused by: {}", source);
    }
}

fn generate_otpauth_uri(name: &str, secret: &str) -> String {
    let label = format!("hotpot:{}", name);
    let params = vec![
        ("secret", secret),
        ("issuer", "hotpot"),
        ("algorithm", "SHA1"),
        ("digits", "6"),
        ("period", "30"),
    ];
    
    let query = params.iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("&");
    
    format!("otpauth://totp/{}?{}", label, query)
}

fn export_qr_code(name: &str, secret: &str) -> Result<(), AppError> {
    use qrcode::{QrCode, render::unicode};

    let uri = generate_otpauth_uri(name, secret);
    println!("Generated URI: {}", uri);
    let code = QrCode::new(uri.as_bytes()).map_err(|e| AppError::new(format!("QR code error: {}", e)))?;
    let qr_string = code.render::<unicode::Dense1x2>()
        .dark_color(unicode::Dense1x2::Light)
        .light_color(unicode::Dense1x2::Dark)
        .build();
    
    println!("\n{}", qr_string);
    Ok(())
}

fn main() {
    let cli = Cli::parse();

    let result = match &cli.command {
        Commands::Add { name } => match prompt_password("Enter the Base32 secret: ") {
            Ok(secret) => save_account(name, &secret).map(|_| println!("Added account: {}", name)),
            Err(err) => Err(AppError::new(err.to_string())),
        },
        Commands::Code { name } => get_account(name).and_then(|account| {
            generate_totp(&account.secret).map(|code| {
                println!("Code for {}: {:06}", name, code);
            })
        }),
        Commands::List => match get_storage() {
            Ok(storage) => {
                if storage.accounts.is_empty() {
                    println!("No accounts configured");
                } else {
                    println!("Configured accounts:");
                    for account in storage.accounts {
                        println!("  {}", account.name);
                    }
                }
                Ok(())
            }
            Err(e) => Err(e),
        },
        Commands::Delete { name } => {
            delete_account(name).map(|_| println!("Deleted account: {}", name))
        },
        Commands::ExportQr { name } => {
            get_account(name).and_then(|account| {
                export_qr_code(name, &account.secret)
            })
        }
    };

    if let Err(err) = result {
        handle_error(err);
        std::process::exit(1);
    }
}
