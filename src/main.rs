use clap::{Parser, Subcommand};
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{Event, KeyCode, KeyEvent, KeyModifiers, poll, read},
    execute,
    style::{Attribute, Color, SetAttribute, SetForegroundColor},
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode, size},
};
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use keyring::Entry;
use rpassword::prompt_password;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::io::{self, Write};
use std::time::{SystemTime, UNIX_EPOCH};

mod totp;
use hotpot::AppError;
use crate::totp::{Account, generate_totp, generate_otpauth_uri};

const SERVICE_NAME: &str = "hotpot";
const STORAGE_KEY: &str = "_hotpot_storage";

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
    /// Interactively search and list accounts
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


fn fuzzy_find() -> Result<(), AppError> {
    let storage = get_storage()?;
    if storage.accounts.is_empty() {
        println!("No accounts configured");
        return Ok(());
    }

    let mut stdout = io::stdout();
    enable_raw_mode()?;
    execute!(stdout, Clear(ClearType::All), Hide)?;

    let mut query = String::new();
    let matcher = SkimMatcherV2::default();
    let mut selected = 0;

    loop {
        // Get terminal size and calculate display area
        let (_, term_height) = size()?;
        let max_display = (term_height - 4) as usize;

        // Filter and score accounts
        let mut matches: Vec<_> = storage
            .accounts
            .iter()
            .filter_map(|account| {
                matcher
                    .fuzzy_match(&account.name, &query)
                    .map(|score| (score, account))
            })
            .collect();
        matches.sort_by_key(|(score, _)| -score);

        // Display UI
        execute!(stdout, MoveTo(0, 0), Clear(ClearType::All))?;
        execute!(stdout, MoveTo(0, 0))?;
        print!("Search: {}_\n", query);

        // Display progress bar
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards");
        let secs_until_next_30 = 30 - (now.as_secs() % 30);
        let progress_percent = ((30 - secs_until_next_30) as f32 / 30.0 * 100.0) as u32;
        let bar_width = 50;
        let filled = (progress_percent as f32 / 100.0 * bar_width as f32) as usize;
        let empty = bar_width - filled;

        execute!(stdout, SetForegroundColor(Color::Green))?;
        print!(
            "[{}{}] {:2}s\n\n",
            "=".repeat(filled),
            " ".repeat(empty),
            secs_until_next_30
        );
        execute!(stdout, SetForegroundColor(Color::Reset))?;

        // Display matches
        for (i, (_, account)) in matches.iter().take(max_display).enumerate() {
            let (term_width, _) = size()?;
            execute!(
                stdout,
                MoveTo(0, i as u16 + 2),
                Clear(ClearType::CurrentLine)
            )?;
            let code = generate_totp(account);
            let max_name_len = (term_width as usize).saturating_sub(10); // Leave room for code
            let display_name = if account.name.len() > max_name_len {
                format!("{}...", &account.name[..max_name_len.saturating_sub(3)])
            } else {
                account.name.clone()
            };

            if i == selected {
                execute!(stdout, SetAttribute(Attribute::Bold))?;
                print!(">{} ", &display_name);
                if let Ok(code) = code {
                    execute!(stdout, MoveTo(term_width.saturating_sub(7), i as u16 + 2))?;
                    print!("{:0width$}", code, width = account.digits as usize);
                }
                execute!(stdout, SetAttribute(Attribute::Reset))?;
            } else {
                print!(" {} ", &display_name);
                if let Ok(code) = code {
                    execute!(stdout, MoveTo(term_width.saturating_sub(7), i as u16 + 2))?;
                    print!("{:0width$}", code, width = account.digits as usize);
                }
            }
            stdout.flush()?;
        }

        // Handle input
        if poll(std::time::Duration::from_millis(50))? {
            match read()? {
                Event::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers: KeyModifiers::CONTROL,
                    ..
                })
                | Event::Key(KeyEvent {
                    code: KeyCode::Esc, ..
                })
                | Event::Key(KeyEvent {
                    code: KeyCode::Char('q'),
                    ..
                }) => {
                    break;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Char(c),
                    ..
                }) => {
                    query.push(c);
                    selected = 0;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Backspace,
                    ..
                }) => {
                    query.pop();
                    selected = 0;
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Up, ..
                }) => {
                    if selected > 0 {
                        selected -= 1;
                    }
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Down,
                    ..
                }) => {
                    if selected + 1 < matches.len().min(max_display) {
                        selected += 1;
                    }
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Enter,
                    ..
                }) => {
                    if let Some((_, account)) = matches.get(selected) {
                        execute!(stdout, Clear(ClearType::All), MoveTo(0, 0), Show)?;
                        disable_raw_mode()?;
                        if let Ok(code) = generate_totp(account) {
                            print!(
                                "Code for {}: {:0width$}\n",
                                account.name,
                                code,
                                width = account.digits as usize
                            );
                            stdout.flush()?;
                        }
                        return Ok(());
                    }
                }
                _ => {}
            }
        }
    }

    execute!(stdout, Show)?;
    disable_raw_mode()?;
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
            generate_totp(&account).map(|code| {
                println!(
                    "Code for {}: {:0width$}",
                    name,
                    code,
                    width = account.digits as usize
                );
            })
        }),
        Commands::List => fuzzy_find(),
        Commands::Delete { name } => {
            delete_account(name).map(|_| println!("Deleted account: {}", name))
        }
        Commands::ExportQr { name } => {
            get_account(name).and_then(|account| export_qr_code(name, &account.secret))
        }
    };

    if let Err(err) = result {
        handle_error(err);
        std::process::exit(1);
    }
}
