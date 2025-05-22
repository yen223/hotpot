use std::{
    cmp::min,
    io::{self, Write},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use arboard::Clipboard;
use crossterm::{
    cursor::{Hide, MoveTo, MoveToNextLine, Show},
    event::{Event, KeyCode, KeyEvent, KeyModifiers, poll, read},
    queue,
    style::{Attribute, Color, Print, SetAttribute, SetBackgroundColor, SetForegroundColor},
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode, size},
};
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use rpassword::prompt_password;

use crate::{AppError, delete_account, get_storage, save_account, totp::generate_totp};

// Define the dashboard modes
enum DashboardMode {
    List,
    Search(String),
    Add,
}

pub fn show() -> Result<(), AppError> {
    let mut stdout = io::stdout();
    enable_raw_mode()?;
    queue!(stdout, Clear(ClearType::All), Hide)?;

    let mut mode = DashboardMode::List;
    let mut selected = 0;
    let matcher = SkimMatcherV2::default();
    let mut name_buffer = String::with_capacity(64);
    // Get storage at the start of each loop iteration
    let mut storage = get_storage()?;
    loop {
        // Get terminal size and calculate display area
        let (term_width, term_height) = size()?;
        let max_display = (term_height - 4) as usize;

        // Clear screen and move to top
        queue!(stdout, MoveTo(0, 0), Clear(ClearType::All))?;

        // Process accounts based on current mode
        let filtered_accounts = match &mode {
            DashboardMode::List => {
                // In list mode, show all accounts in order
                storage.accounts.iter().collect::<Vec<_>>()
            }
            DashboardMode::Search(query) => {
                // In search mode, filter accounts by query
                let mut matches: Vec<_> = storage
                    .accounts
                    .iter()
                    .filter_map(|account| {
                        matcher
                            .fuzzy_match(&account.name, query)
                            .map(|score| (score, account))
                    })
                    .collect();
                matches.sort_by_key(|(score, _)| -score);
                matches.into_iter().map(|(_, acc)| acc).collect()
            }
            DashboardMode::Add => storage.accounts.iter().collect::<Vec<_>>(), // Show accounts in add mode
        };

        // Render the header based on current mode
        match &mode {
            DashboardMode::List => {
                queue!(stdout, Print("[F]ind [A]dd [D]elete [E]xport QR"))?;
            }
            DashboardMode::Search(query) => {
                queue!(stdout, Print(format!("Search (ESC to exit): {}_", query)))?;
            }
            DashboardMode::Add => {
                queue!(
                    stdout,
                    Print("Enter account name (ESC to cancel): "),
                    Print(&name_buffer),
                    Print("_")
                )?;
            }
        }
        queue!(stdout, MoveToNextLine(1))?;

        // Render time-based progress bar
        render_progress_bar(&mut stdout)?;

        // Render account list for applicable modes
        if !filtered_accounts.is_empty() {
            render_account_list(
                &filtered_accounts,
                selected,
                max_display,
                term_width,
                &mut stdout,
            )?;
        }

        // Ensure everything is displayed before handling input
        stdout.flush()?;

        // Process user input
        match handle_input(
            &mut mode,
            &mut selected,
            &filtered_accounts,
            term_height,
            term_width,
            &mut stdout,
            &mut name_buffer,
        )? {
            InputResult::Continue => {
                // Continue the loop
            }
            InputResult::Exit => {
                break;
            }
            InputResult::RefreshStorage => {
                // Storage will be refreshed at the start of the next loop
                storage = get_storage()?;
            }
        }
    }

    queue!(stdout, Show)?;
    disable_raw_mode()?;
    Ok(())
}

fn render_progress_bar(stdout: &mut io::Stdout) -> Result<(), AppError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    let secs_until_next_30 = 30 - (now.as_secs() % 30);
    let bar_width = 30;
    let filled = (30 - secs_until_next_30) as usize;
    let empty = bar_width - filled;

    queue!(stdout, SetForegroundColor(Color::Green))?;
    queue!(
        stdout,
        Print(format!(
            "[{}{}] {:2}s\n\n",
            "=".repeat(filled),
            " ".repeat(empty),
            secs_until_next_30
        ))
    )?;
    queue!(stdout, SetForegroundColor(Color::Reset))?;
    stdout.flush()?;
    Ok(())
}

fn render_account_row(
    account: &crate::totp::Account,
    idx: usize,
    selected: bool,
    term_width: u16,
    copied: bool,
    stdout: &mut io::Stdout,
) -> Result<(), AppError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    let code = generate_totp(account, now);
    let max_width = min(term_width, 64);
    let max_name_len = (max_width as usize).saturating_sub(10); // Leave room for code
    let display_name = if account.name.len() > max_name_len {
        format!("{}...", &account.name[..max_name_len.saturating_sub(3)])
    } else {
        account.name.clone()
    };

    queue!(
        stdout,
        MoveTo(0, idx as u16 + 2),
        Clear(ClearType::CurrentLine)
    )?;

    if selected {
        queue!(
            stdout,
            SetAttribute(Attribute::Bold),
            SetForegroundColor(Color::Black),
            SetBackgroundColor(Color::White)
        )?;
    }
    queue!(stdout, Print(format!("  {} ", &display_name)))?;
    if let Ok(code) = code {
        queue!(
            stdout,
            Print(
                " ".repeat(
                    (max_width as usize)
                        .saturating_sub(7)
                        .saturating_sub(display_name.len())
                        + 2
                )
            ),
            Print(format!("{:0width$}", code, width = account.digits as usize))
        )?;
    }
    queue!(
        stdout,
        SetAttribute(Attribute::Reset),
        SetBackgroundColor(Color::Reset),
        SetForegroundColor(Color::Reset)
    )?;
    if copied {
        queue!(stdout, Print(" [copied]"))?;
    }
    stdout.flush()?;
    Ok(())
}

fn render_account_list(
    accounts: &[&crate::totp::Account],
    selected: usize,
    max_display: usize,
    term_width: u16,
    stdout: &mut io::Stdout,
) -> Result<(), AppError> {
    for (i, account) in accounts.iter().take(max_display).enumerate() {
        render_account_row(account, i, i == selected, term_width, false, stdout)?;
    }
    Ok(())
}

enum InputResult {
    Continue,
    Exit,
    RefreshStorage,
}

fn handle_input(
    mode: &mut DashboardMode,
    selected: &mut usize,
    accounts: &[&crate::totp::Account],
    term_height: u16,
    term_width: u16,
    stdout: &mut io::Stdout,
    name_buffer: &mut String,
) -> Result<InputResult, AppError> {
    if poll(std::time::Duration::from_millis(50))? {
        match read()? {
            Event::Key(KeyEvent {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
                ..
            })
            | Event::Key(KeyEvent {
                code: KeyCode::Char('q'),
                ..
            }) => {
                return Ok(InputResult::Exit);
            }
            Event::Key(KeyEvent {
                code: KeyCode::Esc, ..
            }) => match mode {
                DashboardMode::List => return Ok(InputResult::Exit),
                _ => *mode = DashboardMode::List,
            },
            Event::Key(KeyEvent {
                code: KeyCode::Char(c),
                ..
            }) => match mode {
                DashboardMode::List => match c.to_ascii_lowercase() {
                    'f' => {
                        *mode = DashboardMode::Search(String::new());
                        *selected = 0;
                    }
                    'a' => {
                        *mode = DashboardMode::Add;
                        name_buffer.clear();
                    }
                    'd' => {
                        if let Some(account) = accounts.get(*selected) {
                            return handle_delete_confirmation(account, stdout);
                        }
                    }
                    'e' => {
                        if let Some(account) = accounts.get(*selected) {
                            return handle_export_qr(account, stdout);
                        }
                    }
                    _ => {}
                },
                DashboardMode::Search(query) => {
                    query.push(c);
                    *selected = 0;
                }
                DashboardMode::Add => {
                    name_buffer.push(c);
                }
            },
            Event::Key(KeyEvent {
                code: KeyCode::Backspace,
                ..
            }) => {
                match mode {
                    DashboardMode::Search(query) => {
                        query.pop();
                        *selected = 0;
                    }
                    DashboardMode::Add => {
                        name_buffer.pop();
                    }
                    _ => {}
                }
            }
            Event::Key(KeyEvent {
                code: KeyCode::Up, ..
            }) => {
                if *selected > 0 {
                    *selected -= 1;
                }
            }
            Event::Key(KeyEvent {
                code: KeyCode::Down,
                ..
            }) => {
                let max_items = accounts.len().min(term_height as usize - 4);
                if *selected + 1 < max_items {
                    *selected += 1;
                }
            }
            Event::Key(KeyEvent {
                code: KeyCode::Enter,
                ..
            }) => {
                match mode {
                    DashboardMode::Add => {
                        if !name_buffer.trim().is_empty() {
                            let result = handle_add_mode(stdout, &name_buffer)?;
                            *mode = DashboardMode::List;  // Reset to List mode
                            return Ok(result);
                        }
                    }
                    _ => {
                        if let Some(account) = accounts.get(*selected) {
                            match mode {
                                DashboardMode::List | DashboardMode::Search(_) => {
                                    copy_code_to_clipboard(account, *selected, term_width, stdout)?;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(InputResult::Continue)
}

fn handle_add_mode(stdout: &mut io::Stdout, name: &str) -> Result<InputResult, AppError> {
    // Temporarily restore terminal state
    queue!(stdout, Clear(ClearType::All), MoveTo(0, 0), Show)?;
    stdout.flush()?;
    disable_raw_mode()?;

    if let Ok(secret) = prompt_password("Enter the Base32 secret: ") {
        if let Ok(()) = save_account(name, &secret) {
            queue!(stdout, Print(format!("Added account: {}", name)))?;
        }
    }

    // Restore dashboard state
    enable_raw_mode()?;
    queue!(stdout, Clear(ClearType::All), Hide)?;

    Ok(InputResult::RefreshStorage)
}

fn handle_delete_confirmation(
    account: &crate::totp::Account,
    stdout: &mut io::Stdout,
) -> Result<InputResult, AppError> {
    // Clear only the first line and show cursor
    queue!(
        stdout,
        MoveTo(0, 0),
        Clear(ClearType::CurrentLine),
        Show,
        Print(format!("Delete account '{}'? [y/N] ", account.name))
    )?;
    stdout.flush()?;
    disable_raw_mode()?;

    // Get confirmation
    let mut confirm = String::new();
    io::stdin().read_line(&mut confirm)?;

    let result = if confirm.trim().eq_ignore_ascii_case("y") {
        if let Ok(()) = delete_account(&account.name) {
            // Clear confirmation message
            queue!(stdout, MoveTo(0, 0), Clear(ClearType::CurrentLine))?;
            stdout.flush()?;
            InputResult::RefreshStorage
        } else {
            InputResult::Continue
        }
    } else {
        InputResult::Continue
    };

    // Restore dashboard state
    enable_raw_mode()?;
    queue!(stdout, Hide)?;
    stdout.flush()?;

    Ok(result)
}

fn copy_code_to_clipboard(
    account: &crate::totp::Account,
    selected_idx: usize,
    term_width: u16,
    stdout: &mut io::Stdout,
) -> Result<(), AppError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    if let Ok(code) = generate_totp(account, duration) {
        if let Ok(mut clipboard) = Clipboard::new() {
            let _ = clipboard.set_text(format!("{}", code));
            render_account_row(account, selected_idx, true, term_width, true, stdout)?;
            stdout.flush()?;
            thread::sleep(std::time::Duration::from_secs(1));
            render_account_row(account, selected_idx, true, term_width, false, stdout)?;
            stdout.flush()?;
        }
    }
    Ok(())
}

fn handle_export_qr(
    account: &crate::totp::Account,
    stdout: &mut io::Stdout,
) -> Result<InputResult, AppError> {
    use qrcode::{QrCode, render::unicode};

    // Clear screen and show cursor
    queue!(stdout, Clear(ClearType::All), MoveTo(0, 0), Show)?;
    stdout.flush()?;
    disable_raw_mode()?;

    // Generate the otpauth URI
    let uri = account.generate_uri();
    println!("QR Code for {}", account.name);
    println!("\nGenerated URI: {}\n", uri);

    // Generate and display QR code
    let code = QrCode::new(uri.as_bytes())
        .map_err(|e| AppError::new(format!("QR code error: {}", e)))?;
    let qr_string = code
        .render::<unicode::Dense1x2>()
        .dark_color(unicode::Dense1x2::Light)
        .light_color(unicode::Dense1x2::Dark)
        .build();
    println!("{}\n", qr_string);
    println!("Press Enter to return to dashboard...");

    // Wait for Enter key
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    // Restore dashboard state
    enable_raw_mode()?;
    queue!(stdout, Clear(ClearType::All), Hide)?;
    stdout.flush()?;

    Ok(InputResult::Continue)
}
