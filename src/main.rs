use clap::{Parser, Subcommand};
use keyring::Entry;
use rpassword::prompt_password;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs;
use std::path::Path;
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
    
    /// Use file-backed storage instead of secure keyring storage
    #[arg(short = 'f', long = "file", value_name = "FILE_PATH")]
    file: Option<String>,
    
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Add a new account with secret
    Add {
        /// Account name (e.g., email or service identifier). Optional when using --image (will use name from QR code or prompt)
        name: Option<String>,
        /// Load account from QR code image instead of prompting for secret
        #[arg(long, value_name = "IMAGE_PATH")]
        image: Option<String>,
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

#[derive(Serialize, Deserialize, Default, Clone)]
struct Storage {
    accounts: Vec<Account>,
}

fn get_storage(file_path: Option<&str>) -> Result<Storage, AppError> {
    if let Some(path) = file_path {
        // File-backed storage
        if Path::new(path).exists() {
            let data = fs::read_to_string(path)
                .map_err(|e| AppError::new(format!("Failed to read file {}: {}", path, e)))?;
            Ok(serde_json::from_str(&data)?)
        } else {
            Ok(Storage::default())
        }
    } else {
        // Keyring storage
        let entry = Entry::new(SERVICE_NAME, STORAGE_KEY).map_err(AppError::from)?;

        match entry.get_password() {
            Ok(data) => Ok(serde_json::from_str(&data)?),
            Err(keyring::Error::NoEntry) => Ok(Storage::default()),
            Err(e) => Err(AppError::from(e)),
        }
    }
}

fn save_storage(storage: &Storage, file_path: Option<&str>) -> Result<(), AppError> {
    let data = serde_json::to_string_pretty(storage)?;
    
    if let Some(path) = file_path {
        // File-backed storage
        if let Some(parent) = Path::new(path).parent() {
            fs::create_dir_all(parent)
                .map_err(|e| AppError::new(format!("Failed to create directory: {}", e)))?;
        }
        fs::write(path, data)
            .map_err(|e| AppError::new(format!("Failed to write file {}: {}", path, e)))
    } else {
        // Keyring storage
        Entry::new(SERVICE_NAME, STORAGE_KEY)?
            .set_password(&data)
            .map_err(AppError::from)
    }
}

fn save_account(name: &str, secret: &str, file_path: Option<&str>) -> Result<(), AppError> {
    let mut storage = get_storage(file_path)?;
    if storage.accounts.iter().any(|a| a.name == name) {
        return Err(AppError::new(format!("Account '{}' already exists", name)));
    }
    storage
        .accounts
        .push(Account::new(name.to_string(), secret.to_string()));
    storage.accounts.sort_by(|a, b| a.name.cmp(&b.name));
    save_storage(&storage, file_path)
}

fn get_account(name: &str, file_path: Option<&str>) -> Result<Account, AppError> {
    let storage = get_storage(file_path)?;
    storage
        .accounts
        .iter()
        .find(|a| a.name == name)
        .cloned()
        .ok_or_else(|| AppError::new(format!("Account '{}' not found", name)))
}

fn delete_account(name: &str, file_path: Option<&str>) -> Result<(), AppError> {
    let mut storage = get_storage(file_path)?;
    let initial_len = storage.accounts.len();
    storage.accounts.retain(|a| a.name != name);
    if storage.accounts.len() == initial_len {
        return Err(AppError::new(format!("Account '{}' not found", name)));
    }
    save_storage(&storage, file_path)
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

fn validate_file_path(path: &str) -> Result<(), AppError> {
    let path_obj = Path::new(path);
    
    // Check if parent directory exists or can be created
    if let Some(parent) = path_obj.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)
                .map_err(|e| AppError::new(format!("Cannot create directory '{}': {}", parent.display(), e)))?;
        }
    }
    
    // Check if file is readable/writable if it exists
    if path_obj.exists() {
        if path_obj.is_dir() {
            return Err(AppError::new(format!("'{}' is a directory, not a file", path)));
        }
        
        // Try to read the file to check permissions - but only if it exists
        let metadata = fs::metadata(path_obj)
            .map_err(|e| AppError::new(format!("Cannot access file '{}': {}", path, e)))?;
        
        if !metadata.is_file() {
            return Err(AppError::new(format!("'{}' is not a regular file", path)));
        }
    }
    
    Ok(())
}

fn main() {
    let cli = Cli::parse();
    let file_path = cli.file.as_deref();
    
    // Validate file path if provided
    if let Some(path) = file_path {
        if let Err(err) = validate_file_path(path) {
            handle_error(err);
            std::process::exit(1);
        }
    }

    let result = match &cli.command {
        None => dashboard::show(file_path),
        Some(Commands::Add { name, image }) => {
            if let Some(image_path) = image {
                // Load account from QR code image
                match load_qr_code_from_image(image_path) {
                    Ok(uri) => {
                        println!("Found otpauth URI: {}", uri);
                        match parse_otpauth_uri(&uri) {
                            Ok((default_name, secret, issuer)) => {
                                // Use provided name or prompt for name with default from QR code
                                match if let Some(provided_name) = name {
                                    Ok(provided_name.clone())
                                } else {
                                    prompt_account_name(&default_name)
                                } {
                                    Ok(account_name) => {
                                        save_account(&account_name, &secret, file_path)
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
            } else {
                // Traditional secret input - name is required
                if let Some(account_name) = name {
                    match prompt_password("Enter the Base32 secret: ") {
                        Ok(secret) => save_account(account_name, &secret, file_path).map(|_| println!("Added account: {}", account_name)),
                        Err(err) => Err(AppError::new(err.to_string())),
                    }
                } else {
                    Err(AppError::new("Account name is required when not using --image"))
                }
            }
        }
        Some(Commands::Code { name }) => get_account(name, file_path).and_then(|account| {
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
        Some(Commands::Delete { name }) => {
            delete_account(name, file_path).map(|_| println!("Deleted account: {}", name))
        }
        Some(Commands::ExportQr { name }) => {
            get_account(name, file_path).and_then(|account| export_qr_code(name, &account.secret))
        }
    };

    if let Err(err) = result {
        handle_error(err);
        std::process::exit(1);
    }
}
