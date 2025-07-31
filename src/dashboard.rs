use std::{
    cmp::min,
    io::{self, Write},
    time::{SystemTime, UNIX_EPOCH},
};

use arboard::Clipboard;
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{Event, KeyCode, KeyEvent, KeyModifiers, poll, read},
    queue,
    style::{Attribute, Color, Print, SetAttribute, SetBackgroundColor, SetForegroundColor},
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode, size},
};
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use rpassword::prompt_password;

use crate::{AppError, delete_account, get_storage, save_account, totp::generate_totp};

// Screen buffer for double buffering
struct ScreenBuffer {
    lines: Vec<BufferLine>,
    width: u16,
    height: u16,
}

#[derive(Clone)]
struct BufferLine {
    content: String,
    is_highlighted: bool,
}

impl ScreenBuffer {
    fn new(width: u16, height: u16) -> Self {
        Self {
            lines: vec![
                BufferLine {
                    content: String::new(),
                    is_highlighted: false
                };
                height as usize
            ],
            width,
            height,
        }
    }

    fn clear(&mut self) {
        for line in &mut self.lines {
            line.content.clear();
            line.is_highlighted = false;
        }
    }

    fn write_line(&mut self, row: u16, content: String) {
        if row < self.height {
            self.lines[row as usize].content = content;
            self.lines[row as usize].is_highlighted = false;
        }
    }

    fn write_highlighted_line(&mut self, row: u16, content: String) {
        if row < self.height {
            self.lines[row as usize].content = content;
            self.lines[row as usize].is_highlighted = true;
        }
    }

    fn flush_to_screen(&self, stdout: &mut io::Stdout) -> Result<(), AppError> {
        queue!(stdout, MoveTo(0, 0), Clear(ClearType::All))?;
        for (row, line) in self.lines.iter().enumerate() {
            if !line.content.is_empty() {
                queue!(stdout, MoveTo(0, row as u16))?;

                if line.is_highlighted {
                    queue!(
                        stdout,
                        SetAttribute(Attribute::Bold),
                        SetForegroundColor(Color::Black),
                        SetBackgroundColor(Color::White),
                        Print(&line.content),
                        SetAttribute(Attribute::Reset),
                        SetForegroundColor(Color::Reset),
                        SetBackgroundColor(Color::Reset)
                    )?;
                } else {
                    queue!(stdout, Print(&line.content))?;
                }
            }
        }
        stdout.flush()?;
        Ok(())
    }

    fn render_header(&mut self, mode: &DashboardMode, name_buffer: &str) {
        let header = match mode {
            DashboardMode::List => "[F]ind [A]dd [D]elete [E]xport QR [Q]uit".to_string(),
            DashboardMode::Search(query) => format!("Search (ESC to exit): {}_", query),
            DashboardMode::Add => format!("Enter account name (ESC to cancel): {}_", name_buffer),
            DashboardMode::AddMethod => {
                if cfg!(target_os = "macos") {
                    "Choose add method: [S]creenshot [M]anual (ESC to cancel)".to_string()
                } else {
                    "Choose add method: [M]anual (ESC to cancel)".to_string()
                }
            },
        };
        self.write_line(0, header);
    }

    fn render_progress_bar(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards");
        let secs_until_next_30 = 30 - (now.as_secs() % 30);
        let bar_width = 30;
        let filled = (30 - secs_until_next_30) as usize;
        let empty = bar_width - filled;

        let progress_line = format!(
            "|{}{}| {:2}s",
            "█".repeat(filled),
            " ".repeat(empty),
            secs_until_next_30
        );
        self.write_line(2, progress_line);
    }

    fn render_account_line(
        &mut self,
        account: &crate::totp::Account,
        row: u16,
        selected: bool,
    ) -> Result<(), AppError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards");
        let code = generate_totp(account, now)?;

        let max_width = min(self.width, 64);
        let max_name_len = (max_width as usize).saturating_sub(8); // Leave room for code
        let display_name = if account.name.len() > max_name_len {
            format!("{}...", &account.name[..max_name_len.saturating_sub(3)])
        } else {
            account.name.clone()
        };

        let spacing = " ".repeat(
            (max_width as usize)
                .saturating_sub(6)
                .saturating_sub(display_name.len())
                + 2,
        );

        let line = format!(
            " {} {}{:0width$} ",
            display_name,
            spacing,
            code,
            width = account.digits as usize
        );

        if selected {
            self.write_highlighted_line(row, line);
        } else {
            self.write_line(row, line);
        }
        Ok(())
    }
}

// Define the dashboard modes
enum DashboardMode {
    List,
    Search(String),
    Add,
    AddMethod,
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

    // Initialize screen buffer
    let (mut term_width, mut term_height) = size()?;
    let mut buffer = ScreenBuffer::new(term_width, term_height);

    loop {
        // Check if terminal size changed
        let (new_width, new_height) = size()?;
        if new_width != term_width || new_height != term_height {
            term_width = new_width;
            term_height = new_height;
            buffer = ScreenBuffer::new(term_width, term_height);
        }
        let max_display = (term_height - 4) as usize;

        // Clear buffer for new frame
        buffer.clear();

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
            DashboardMode::AddMethod => storage.accounts.iter().collect::<Vec<_>>(), // Show accounts in add method mode
        };

        // Render to buffer
        buffer.render_header(&mode, &name_buffer);
        buffer.render_progress_bar();

        // Render account list to buffer
        if !filtered_accounts.is_empty() {
            let display_count = min(filtered_accounts.len(), max_display);
            for (idx, account) in filtered_accounts.iter().take(display_count).enumerate() {
                let is_selected = idx == selected;
                buffer.render_account_line(account, 4 + idx as u16, is_selected)?;
            }
        }

        // Flush buffer to screen
        buffer.flush_to_screen(&mut stdout)?;

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
            InputResult::RefreshStorageAndResetMode => {
                // Storage will be refreshed and mode reset to List
                storage = get_storage()?;
                mode = DashboardMode::List;
            }
        }
    }

    queue!(stdout, Show)?;
    disable_raw_mode()?;
    Ok(())
}

enum InputResult {
    Continue,
    Exit,
    RefreshStorage,
    RefreshStorageAndResetMode,
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
    if poll(std::time::Duration::from_millis(250))? {
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
                        *mode = DashboardMode::AddMethod;
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
                DashboardMode::AddMethod => match c.to_ascii_lowercase() {
                    's' if cfg!(target_os = "macos") => {
                        return handle_screenshot_add(stdout);
                    }
                    'm' => {
                        *mode = DashboardMode::Add;
                        name_buffer.clear();
                    }
                    _ => {}
                }
            },
            Event::Key(KeyEvent {
                code: KeyCode::Backspace,
                ..
            }) => match mode {
                DashboardMode::Search(query) => {
                    query.pop();
                    *selected = 0;
                }
                DashboardMode::Add => {
                    name_buffer.pop();
                }
                _ => {}
            },
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
                            return handle_add_mode(stdout, &name_buffer);
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

    Ok(InputResult::RefreshStorageAndResetMode)
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
        SetForegroundColor(Color::Red),
        Print(format!("Delete account '{}'? [y/N] ", account.name)),
        SetForegroundColor(Color::Reset)
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
    _selected_idx: usize,
    _term_width: u16,
    _stdout: &mut io::Stdout,
) -> Result<(), AppError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    if let Ok(code) = generate_totp(account, duration) {
        if let Ok(mut clipboard) = Clipboard::new() {
            let _ = clipboard.set_text(format!("{}", code));
            // Visual feedback is now handled by the main rendering loop
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
    let code =
        QrCode::new(uri.as_bytes()).map_err(|e| AppError::new(format!("QR code error: {}", e)))?;
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

#[cfg(target_os = "macos")]
fn handle_screenshot_add(stdout: &mut io::Stdout) -> Result<InputResult, AppError> {
    use std::process::Command;
    use std::fs;

    // Clear screen and show cursor
    queue!(stdout, Clear(ClearType::All), MoveTo(0, 0), Show)?;
    stdout.flush()?;
    disable_raw_mode()?;

    println!("Screenshot mode - select area to capture QR code");
    println!("Position your cursor and drag to select the QR code area...");

    // Create temporary file path
    let temp_path = "/tmp/hotpot_screenshot.png";

    // Call screencapture with interactive selection
    let output = Command::new("screencapture")
        .arg("-i")  // Interactive selection
        .arg("-r")  // No drop shadow
        .arg(temp_path)
        .output()
        .map_err(|e| AppError::new(format!("Failed to call screencapture: {}", e)))?;

    if !output.status.success() {
        println!("Screenshot cancelled or failed");
        println!("Press Enter to return to dashboard...");
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        
        // Restore dashboard state
        enable_raw_mode()?;
        queue!(stdout, Clear(ClearType::All), Hide)?;
        stdout.flush()?;
        return Ok(InputResult::Continue);
    }

    // Read and decode QR code from screenshot
    match decode_qr_from_image(temp_path) {
        Ok(qr_data) => {
            // Clean up temp file
            let _ = fs::remove_file(temp_path);
            
            // Try to parse as otpauth URI
            if let Some(extracted_name) = extract_account_from_otpauth(&qr_data) {
                if let Some(secret) = extract_secret_from_otpauth(&qr_data) {
                    // Prompt for account name with default
                    println!("Enter account name (press Enter for default) [{}]: ", extracted_name);
                    let mut input = String::new();
                    io::stdin().read_line(&mut input)?;
                    
                    let account_name = input.trim();
                    let final_name = if account_name.is_empty() {
                        extracted_name
                    } else {
                        account_name.to_string()
                    };
                    
                    match save_account(&final_name, &secret) {
                        Ok(()) => {
                            println!("Successfully added account: {}", final_name);
                        }
                        Err(e) => {
                            println!("Failed to save account: {}", e);
                        }
                    }
                } else {
                    println!("Could not extract secret from QR code");
                }
            } else {
                println!("QR code does not appear to contain a valid TOTP setup");
                println!("QR code contents: {}", qr_data);
            }
        }
        Err(e) => {
            // Clean up temp file
            let _ = fs::remove_file(temp_path);
            println!("Failed to decode QR code: {}", e);
        }
    }

    println!("Press Enter to return to dashboard...");
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    // Restore dashboard state
    enable_raw_mode()?;
    queue!(stdout, Clear(ClearType::All), Hide)?;
    stdout.flush()?;

    Ok(InputResult::RefreshStorageAndResetMode)
}

fn decode_qr_from_image(image_path: &str) -> Result<String, AppError> {
    use image::ImageReader;
    use rqrr::PreparedImage;
    
    // Load the image
    let img = ImageReader::open(image_path)
        .map_err(|e| AppError::new(format!("Failed to open image: {}", e)))?
        .decode()
        .map_err(|e| AppError::new(format!("Failed to decode image: {}", e)))?;

    // Convert to luma (grayscale) for QR detection
    let luma_img = img.to_luma8();

    // Prepare image for QR detection
    let mut prepared = PreparedImage::prepare(luma_img);
    
    // Try to find and decode QR codes
    let grids = prepared.detect_grids();
    if grids.is_empty() {
        return Err(AppError::new("No QR code found in image".to_string()));
    }

    // Decode the first QR code found
    let (_, content) = grids[0].decode()
        .map_err(|e| AppError::new(format!("Failed to decode QR code: {:?}", e)))?;

    Ok(content)
}

fn extract_account_from_otpauth(uri: &str) -> Option<String> {
    if !uri.starts_with("otpauth://totp/") {
        return None;
    }
    
    // Extract account name from URI path
    let path_start = uri.find("otpauth://totp/")?;
    let path = &uri[path_start + 15..]; // Skip "otpauth://totp/"
    
    if let Some(query_start) = path.find('?') {
        let account_part = &path[..query_start];
        // URL decode and extract just the account name
        Some(urlencoding::decode(account_part).ok()?.to_string())
    } else {
        Some(urlencoding::decode(path).ok()?.to_string())
    }
}

fn extract_secret_from_otpauth(uri: &str) -> Option<String> {
    use url::Url;
    
    let parsed = Url::parse(uri).ok()?;
    let pairs: std::collections::HashMap<_, _> = parsed.query_pairs().collect();
    pairs.get("secret").map(|s| s.to_string())
}
