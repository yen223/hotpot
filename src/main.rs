use base32::{Alphabet, decode};
use clap::{Parser, Subcommand};
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{Event, KeyCode, KeyEvent, poll},
    execute,
    style::{Color, SetForegroundColor},
    terminal::{Clear, ClearType, size},
};
use hmac::{Hmac, Mac};
use keyring::Entry;
use rpassword::prompt_password;
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use std::error::Error;
use std::fmt;
use std::io::{self, Write};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
    /// Watch and continuously update codes for all accounts
    Watch,
}

#[derive(Serialize, Deserialize, Default)]
struct Storage {
    accounts: Vec<Account>,
}

#[derive(Serialize, Deserialize, Clone)]
struct Account {
    name: String,
    secret: String,
    #[serde(default = "default_issuer")]
    issuer: String,
    #[serde(default = "default_algorithm")]
    algorithm: String,
    #[serde(default = "default_digits")]
    digits: u32,
    #[serde(default = "default_period")]
    period: u32,
}

fn default_issuer() -> String {
    "hotpot".to_string()
}
fn default_algorithm() -> String {
    "SHA1".to_string()
}
fn default_digits() -> u32 {
    6
}
fn default_period() -> u32 {
    30
}

impl Account {
    fn new(name: String, secret: String) -> Self {
        Self {
            name,
            secret,
            issuer: default_issuer(),
            algorithm: default_algorithm(),
            digits: default_digits(),
            period: default_period(),
        }
    }

    fn generate_uri(&self) -> String {
        let label = format!("{}:{}", self.issuer, self.name);
        let digits = self.digits.to_string();
        let period = self.period.to_string();
        let params = vec![
            ("secret", &self.secret),
            ("issuer", &self.issuer),
            ("algorithm", &self.algorithm),
            ("digits", &digits),
            ("period", &period),
        ];

        let query = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");

        format!("otpauth://totp/{}?{}", label, query)
    }
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

fn generate_totp(account: &Account) -> Result<u32, AppError> {
    let secret_bytes = match decode(Alphabet::RFC4648 { padding: false }, &account.secret) {
        Some(bytes) => bytes,
        None => return Err(AppError::new("Bytes could not be decoded")),
    };

    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time is before Unix epoch");
    let counter = duration.as_secs() / u64::from(account.period);

    let mut mac = match account.algorithm.as_str() {
        "SHA1" => {
            Hmac::<Sha1>::new_from_slice(&secret_bytes).expect("HMAC can take key of any size")
        }
        _ => return Err(AppError::new("Unsupported algorithm")), // Add support for SHA256/SHA512 if needed
    };

    mac.update(&counter.to_be_bytes());
    let result = mac.finalize().into_bytes();

    let offset = (result[19] & 0xf) as usize;
    let binary = ((u32::from(result[offset]) & 0x7f) << 24)
        | ((u32::from(result[offset + 1]) & 0xff) << 16)
        | ((u32::from(result[offset + 2]) & 0xff) << 8)
        | (u32::from(result[offset + 3]) & 0xff);

    let modulus = 10u32.pow(account.digits);
    Ok(binary % modulus)
}

fn handle_error(err: AppError) {
    eprintln!("Error: {}", err);
    if let Some(source) = err.source() {
        eprintln!("Caused by: {}", source);
    }
}

fn generate_otpauth_uri(name: &str, secret: &str) -> String {
    Account::new(name.to_string(), secret.to_string()).generate_uri()
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

fn watch_codes() -> Result<(), AppError> {
    let mut stdout = io::stdout();
    execute!(stdout, Clear(ClearType::All), Hide)
        .map_err(|e| AppError::new(format!("Terminal error: {}", e)))?;
    let storage = get_storage()?;

    if storage.accounts.is_empty() {
        println!("No accounts configured");
        execute!(stdout, Show)?;
        return Ok(());
    }

    // Calculate maximum width for name column
    let max_name_width = storage
        .accounts
        .iter()
        .map(|a| a.name.len())
        .max()
        .unwrap_or(0);

    let (_, term_height) = size().map_err(|e| AppError::new(format!("Terminal error: {}", e)))?;

    loop {
        if poll(Duration::from_millis(100))? {
            if let Event::Key(KeyEvent {
                code: KeyCode::Char('q'),
                ..
            }) = crossterm::event::read()?
            {
                execute!(stdout, Show)?;
                return Ok(());
            }
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards");
        let secs_until_next_30 = 30 - (now.as_secs() % 30);
        let progress_percent = ((30 - secs_until_next_30) as f32 / 30.0 * 100.0) as u32;

        execute!(stdout, MoveTo(0, 0))
            .map_err(|e| AppError::new(format!("Terminal error: {}", e)))?;
        println!("Watching TOTP codes (press 'q' to exit):\n");

        // Print table header
        println!("┌─{}─┬─{}─┐", "─".repeat(max_name_width), "─".repeat(6));
        println!(
            "│ {:<width$} │ {:<6} │",
            "Account",
            "Code",
            width = max_name_width
        );
        println!("├─{}─┼─{}─┤", "─".repeat(max_name_width), "─".repeat(6));

        for account in &storage.accounts {
            match generate_totp(account) {
                Ok(code) => println!(
                    "│ {:<width$} │ {:0>6} │",
                    account.name,
                    code,
                    width = max_name_width
                ),
                Err(e) => println!(
                    "│ {:<width$} │ ERR:{} │",
                    account.name,
                    e.message.chars().take(3).collect::<String>(),
                    width = max_name_width
                ),
            }
        }

        // Print table footer
        println!("└─{}─┴─{}─┘", "─".repeat(max_name_width), "─".repeat(6));
        println!();

        // Draw progress bar at the bottom of the terminal
        execute!(stdout, MoveTo(0, term_height - 2), Clear(ClearType::CurrentLine))?;
        let bar_width = 50;
        let filled = (progress_percent as f32 / 100.0 * bar_width as f32) as usize;
        let empty = bar_width - filled;

        execute!(stdout, SetForegroundColor(Color::Green))?;
        print!(
            "[{}{}] {:2}s",
            "=".repeat(filled),
            " ".repeat(empty),
            secs_until_next_30
        );
        execute!(stdout, SetForegroundColor(Color::Reset))?;

        stdout
            .flush()
            .map_err(|e| AppError::new(format!("Terminal error: {}", e)))?;
    }
}

fn main() {
    let cli = Cli::parse();

    let result = match &cli.command {
        Commands::Add { name } => match prompt_password("Enter the Base32 secret: ") {
            Ok(secret) => save_account(name, &secret).map(|_| println!("Added account: {}", name)),
            Err(err) => Err(AppError::new(err.to_string())),
        },
        Commands::Code { name } => get_account(name).and_then(|account| {
            generate_totp(&account).map(|code| {
                println!(
                    "Code for {}: {:0width$}",
                    name,
                    code,
                    width = account.digits as usize
                );
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
        }
        Commands::ExportQr { name } => {
            get_account(name).and_then(|account| export_qr_code(name, &account.secret))
        }
        Commands::Watch => watch_codes(),
    };

    if let Err(err) = result {
        handle_error(err);
        std::process::exit(1);
    }
}
