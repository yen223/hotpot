use clap::{Parser, Subcommand};
use keyring::Entry;
use rpassword::prompt_password;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};
use std::io::{self, Write};

mod dashboard;
mod totp;
use crate::totp::{Account, generate_otpauth_uri, generate_totp};
use hotpot::AppError;

const SERVICE_NAME: &str = "hotpot";
const STORAGE_KEY: &str = "_hotpot_storage";

#[derive(Parser)]
#[command(name = "hotpot")]
#[command(about = "A simple CLI for TOTP-based 2FA", long_about = None)]
struct Cli {
    /// Load account from QR code image
    #[arg(short = 'l', long = "load-image", value_name = "IMAGE_PATH")]
    load_image: Option<String>,
    
    #[command(subcommand)]
    command: Option<Commands>,
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
    storage
        .accounts
        .push(Account::new(name.to_string(), secret.to_string()));
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

fn handle_error(err: AppError) {
    eprintln!("Error: {}", err);
    if let Some(source) = err.source() {
        eprintln!("Caused by: {}", source);
    }
}

fn export_qr_code(name: &str, secret: &str) -> Result<(), AppError> {
    use qrcode::{QrCode, render::unicode};

    let uri = generate_otpauth_uri(name, secret);
    println!("Generated URI: {}", uri);
    let code =
        QrCode::new(uri.as_bytes()).map_err(|e| AppError::new(format!("QR code error: {}", e)))?;
    let qr_string = code
        .render::<unicode::Dense1x2>()
        .dark_color(unicode::Dense1x2::Light)
        .light_color(unicode::Dense1x2::Dark)
        .build();

    println!("\n{}", qr_string);
    Ok(())
}

fn load_qr_code_from_image(image_path: &str) -> Result<String, AppError> {
    use image::ImageReader;
    use rqrr::PreparedImage;

    // Load and decode the image
    let img = ImageReader::open(image_path)
        .map_err(|e| AppError::new(format!("Failed to open image: {}", e)))?
        .decode()
        .map_err(|e| AppError::new(format!("Failed to decode image: {}", e)))?;

    // Convert to luma (grayscale) for QR code detection
    let luma_img = img.to_luma8();
    let mut prepared_img = PreparedImage::prepare(luma_img);
    
    // Find and decode QR codes
    let grids = prepared_img.detect_grids();
    if grids.is_empty() {
        return Err(AppError::new("No QR code found in image"));
    }

    // Decode the first QR code found
    let (_, content) = grids[0].decode()
        .map_err(|e| AppError::new(format!("Failed to decode QR code: {:?}", e)))?;

    Ok(content)
}

fn parse_otpauth_uri(uri: &str) -> Result<(String, String, String), AppError> {
    if !uri.starts_with("otpauth://totp/") {
        return Err(AppError::new("Invalid otpauth URI format"));
    }

    // Parse the URI manually
    let url = url::Url::parse(uri)
        .map_err(|e| AppError::new(format!("Failed to parse URI: {}", e)))?;

    // Extract account name from path
    let path = url.path().trim_start_matches('/');
    let account_name = if path.contains(':') {
        // Format: issuer:account or account
        path.split(':').last().unwrap_or(path).to_string()
    } else {
        path.to_string()
    };

    // Extract secret from query parameters
    let mut secret = String::new();
    let mut issuer = String::new();
    
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "secret" => secret = value.to_string(),
            "issuer" => issuer = value.to_string(),
            _ => {}
        }
    }

    if secret.is_empty() {
        return Err(AppError::new("No secret found in otpauth URI"));
    }

    // Use issuer if available, otherwise use a default
    if issuer.is_empty() {
        issuer = "Unknown".to_string();
    }

    Ok((account_name, secret, issuer))
}

fn prompt_account_name(default: &str) -> Result<String, AppError> {
    print!("Enter account name [{}]: ", default);
    io::stdout().flush().map_err(AppError::from)?;
    
    let mut input = String::new();
    io::stdin().read_line(&mut input).map_err(AppError::from)?;
    
    let name = input.trim();
    if name.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(name.to_string())
    }
}

fn main() {
    let cli = Cli::parse();

    let result = match (&cli.load_image, &cli.command) {
        (Some(image_path), None) => {
            // Load account from QR code image
            match load_qr_code_from_image(image_path) {
                Ok(uri) => {
                    println!("Found otpauth URI: {}", uri);
                    match parse_otpauth_uri(&uri) {
                        Ok((default_name, secret, issuer)) => {
                            match prompt_account_name(&default_name) {
                                Ok(account_name) => {
                                    save_account(&account_name, &secret)
                                        .map(|_| println!("Added account: {} (from {})", account_name, issuer))
                                }
                                Err(e) => Err(e),
                            }
                        }
                        Err(e) => Err(e),
                    }
                }
                Err(e) => Err(e),
            }
        }
        (Some(_), Some(_)) => {
            Err(AppError::new("Cannot use --load-image with subcommands"))
        }
        (None, None) => dashboard::show(),
        (None, Some(Commands::Add { name })) => match prompt_password("Enter the Base32 secret: ") {
            Ok(secret) => save_account(name, &secret).map(|_| println!("Added account: {}", name)),
            Err(err) => Err(AppError::new(err.to_string())),
        },
        (None, Some(Commands::Code { name })) => get_account(name).and_then(|account| {
            let duration = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("System time is before Unix epoch");
            generate_totp(&account, duration).map(|code| {
                println!(
                    "Code for {}: {:0width$}",
                    name,
                    code,
                    width = account.digits as usize
                );
            })
        }),
        (None, Some(Commands::Delete { name })) => {
            delete_account(name).map(|_| println!("Deleted account: {}", name))
        }
        (None, Some(Commands::ExportQr { name })) => {
            get_account(name).and_then(|account| export_qr_code(name, &account.secret))
        }
    };

    if let Err(err) = result {
        handle_error(err);
        std::process::exit(1);
    }
}
