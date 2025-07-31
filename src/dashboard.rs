use std::{
    cmp::min,
    collections::HashMap,
    io::{self, Write},
    time::{Duration, SystemTime, UNIX_EPOCH},
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
    copied_split_pos: Option<usize>, // Position where "copied" text starts for special rendering
}

impl ScreenBuffer {
    fn new(width: u16, height: u16) -> Self {
        Self {
            lines: vec![
                BufferLine {
                    content: String::new(),
                    is_highlighted: false,
                    copied_split_pos: None,
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
            line.copied_split_pos = None;
        }
    }

    fn write_line(&mut self, row: u16, content: String) {
        self.write_line_with_style(row, content, false, None);
    }

    fn write_highlighted_line(&mut self, row: u16, content: String) {
        self.write_line_with_style(row, content, true, None);
    }
    
    fn write_highlighted_line_with_copied(&mut self, row: u16, content: String, split_pos: usize) {
        self.write_line_with_style(row, content, true, Some(split_pos));
    }

    fn write_line_with_style(&mut self, row: u16, content: String, highlighted: bool, copied_split: Option<usize>) {
        if row < self.height {
            self.lines[row as usize].content = content;
            self.lines[row as usize].is_highlighted = highlighted;
            self.lines[row as usize].copied_split_pos = copied_split;
        }
    }

    fn flush_to_screen(&self, stdout: &mut io::Stdout) -> Result<(), AppError> {
        queue!(stdout, MoveTo(0, 0), Clear(ClearType::All))?;
        for (row, line) in self.lines.iter().enumerate() {
            if !line.content.is_empty() {
                queue!(stdout, MoveTo(0, row as u16))?;
                self.render_line_content(stdout, line)?;
            }
        }
        stdout.flush()?;
        Ok(())
    }

    fn render_line_content(&self, stdout: &mut io::Stdout, line: &BufferLine) -> Result<(), AppError> {
        if line.is_highlighted {
            if let Some(split_pos) = line.copied_split_pos {
                self.render_highlighted_with_copied(stdout, &line.content, split_pos)
            } else {
                self.render_highlighted_content(stdout, &line.content)
            }
        } else {
            self.render_normal_content(stdout, &line.content)
        }
    }

    fn render_highlighted_with_copied(&self, stdout: &mut io::Stdout, content: &str, split_pos: usize) -> Result<(), AppError> {
        let (highlighted_part, copied_part) = content.split_at(split_pos);
        queue!(
            stdout,
            SetAttribute(Attribute::Bold),
            SetForegroundColor(Color::Black),
            SetBackgroundColor(Color::White),
            Print(highlighted_part),
            SetAttribute(Attribute::Reset),
            SetForegroundColor(Color::Reset),
            SetBackgroundColor(Color::Reset),
            Print(copied_part)
        )?;
        Ok(())
    }

    fn render_highlighted_content(&self, stdout: &mut io::Stdout, content: &str) -> Result<(), AppError> {
        queue!(
            stdout,
            SetAttribute(Attribute::Bold),
            SetForegroundColor(Color::Black),
            SetBackgroundColor(Color::White),
            Print(content),
            SetAttribute(Attribute::Reset),
            SetForegroundColor(Color::Reset),
            SetBackgroundColor(Color::Reset)
        )?;
        Ok(())
    }

    fn render_normal_content(&self, stdout: &mut io::Stdout, content: &str) -> Result<(), AppError> {
        queue!(stdout, Print(content))?;
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
        let subsec_progress = now.as_millis() % 1000;
        
        let bar_width = 30;
        let total_progress = (30 - secs_until_next_30) as f64 + (subsec_progress as f64 / 1000.0);
        let filled_exact = total_progress;
        let filled_full = filled_exact.floor() as usize;
        let partial = filled_exact - filled_full as f64;
        
        let blocks = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉", "█"];
        let partial_block = if filled_full < bar_width {
            blocks[(partial * 8.0) as usize]
        } else {
            ""
        };
        
        let remaining = if filled_full < bar_width {
            bar_width - filled_full - if partial_block.is_empty() { 0 } else { 1 }
        } else {
            0
        };

        let progress_line = format!(
            "|{}{}{}| {:2}s",
            "█".repeat(filled_full),
            partial_block,
            " ".repeat(remaining),
            secs_until_next_30
        );
        self.write_line(2, progress_line);
    }

    fn render_account_line(
        &mut self,
        account: &crate::totp::Account,
        row: u16,
        selected: bool,
        copied_state: &CopiedState,
    ) -> Result<(), AppError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards");
        let code = generate_totp(account, now)?;

        let max_width = min(self.width, 64);
        let copied_text = "  copied";
        let copied_indicator = if copied_state.is_recently_copied(&account.name) {
            copied_text
        } else {
            ""
        };
        
        // Always reserve space for " copied" to keep codes aligned
        let code_str = format!("{:0width$}", code, width = account.digits as usize);
        let max_name_len = (max_width as usize).saturating_sub(code_str.len() + copied_text.len() + 3); // 3 for padding
        
        let display_name = if account.name.len() > max_name_len {
            format!("{}...", &account.name[..max_name_len.saturating_sub(3)])
        } else {
            account.name.clone()
        };

        let spacing = " ".repeat(
            (max_width as usize)
                .saturating_sub(1) // left padding
                .saturating_sub(display_name.len())
                .saturating_sub(code_str.len())
                .saturating_sub(copied_text.len()) // Always reserve space for " copied"
                .saturating_sub(1) // right padding
        );

        let line = format!(
            " {} {}{}{} ",
            display_name,
            spacing,
            code_str,
            copied_indicator
        );

        if selected {
            if copied_indicator.is_empty() {
                self.write_highlighted_line(row, line);
            } else {
                // Split at the position where " copied" begins
                let split_pos = line.len() - copied_indicator.len();
                self.write_highlighted_line_with_copied(row, line, split_pos);
            }
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

// Track recently copied accounts
struct CopiedState {
    accounts: HashMap<String, SystemTime>,
}

impl CopiedState {
    fn new() -> Self {
        Self {
            accounts: HashMap::new(),
        }
    }

    fn mark_copied(&mut self, account_name: &str) {
        self.accounts.insert(account_name.to_string(), SystemTime::now());
    }

    fn is_recently_copied(&self, account_name: &str) -> bool {
        if let Some(&copied_time) = self.accounts.get(account_name) {
            if let Ok(elapsed) = SystemTime::now().duration_since(copied_time) {
                return elapsed < Duration::from_secs(2); // Show "copied" for 2 seconds
            }
        }
        false
    }

    fn cleanup_old_entries(&mut self) {
        let now = SystemTime::now();
        self.accounts.retain(|_, &mut copied_time| {
            now.duration_since(copied_time)
                .map(|elapsed| elapsed < Duration::from_secs(2))
                .unwrap_or(false)
        });
    }
}

fn get_filtered_accounts<'a>(
    storage: &'a crate::Storage,
    mode: &DashboardMode,
    matcher: &SkimMatcherV2,
) -> Vec<&'a crate::totp::Account> {
    match mode {
        DashboardMode::List => {
            storage.accounts.iter().collect()
        }
        DashboardMode::Search(query) => {
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
        DashboardMode::Add | DashboardMode::AddMethod => {
            storage.accounts.iter().collect()
        }
    }
}

pub fn show() -> Result<(), AppError> {
    let mut stdout = io::stdout();
    enable_raw_mode()?;
    queue!(stdout, Clear(ClearType::All), Hide)?;

    let mut mode = DashboardMode::List;
    let mut selected = 0;
    let matcher = SkimMatcherV2::default();
    let mut name_buffer = String::with_capacity(64);
    let mut copied_state = CopiedState::new();
    // Get storage at the start of each loop iteration
    let mut storage = get_storage()?;

    // Initialize screen buffer
    let (term_width, term_height) = size()?;
    let mut buffer = ScreenBuffer::new(term_width, term_height);

    loop {
        // Check if terminal size changed
        let (new_width, new_height) = size()?;
        if new_width != buffer.width || new_height != buffer.height {
            buffer = ScreenBuffer::new(new_width, new_height);
        }
        let max_display = (buffer.height - 4) as usize;

        // Clear buffer for new frame
        buffer.clear();
        
        // Clean up old copied entries
        copied_state.cleanup_old_entries();

        let filtered_accounts = get_filtered_accounts(&storage, &mode, &matcher);

        // Render to buffer
        buffer.render_header(&mode, &name_buffer);
        buffer.render_progress_bar();

        // Render account list to buffer
        for (idx, account) in filtered_accounts.iter().take(max_display).enumerate() {
            let is_selected = idx == selected;
            buffer.render_account_line(account, 4 + idx as u16, is_selected, &copied_state)?;
        }

        // Flush buffer to screen
        buffer.flush_to_screen(&mut stdout)?;

        // Process user input
        match handle_input(
            &mut mode,
            &mut selected,
            &filtered_accounts,
            buffer.height,
            buffer.width,
            &mut stdout,
            &mut name_buffer,
            &mut copied_state,
        )? {
            InputResult::Continue => {
                // Continue the loop
            }
            InputResult::Exit => {
                queue!(stdout, Clear(ClearType::All), MoveTo(0, 0))?;
                stdout.flush()?;
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
    copied_state: &mut CopiedState,
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
            }) => {
                return handle_char_input(c, mode, selected, accounts, stdout, name_buffer);
            }
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
                                    copy_code_to_clipboard(account, *selected, term_width, stdout, copied_state)?;
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

fn handle_char_input(
    c: char,
    mode: &mut DashboardMode,
    selected: &mut usize,
    accounts: &[&crate::totp::Account],
    stdout: &mut io::Stdout,
    name_buffer: &mut String,
) -> Result<InputResult, AppError> {
    match mode {
        DashboardMode::List => handle_list_mode_char(c, mode, selected, accounts, stdout),
        DashboardMode::Search(query) => handle_search_mode_char(c, query, selected),
        DashboardMode::Add => handle_add_mode_char(c, name_buffer),
        DashboardMode::AddMethod => handle_add_method_mode_char(c, mode, stdout, name_buffer),
    }
}

fn handle_list_mode_char(
    c: char,
    mode: &mut DashboardMode,
    selected: &mut usize,
    accounts: &[&crate::totp::Account],
    stdout: &mut io::Stdout,
) -> Result<InputResult, AppError> {
    match c.to_ascii_lowercase() {
        'f' => {
            *mode = DashboardMode::Search(String::new());
            *selected = 0;
            Ok(InputResult::Continue)
        }
        'a' => {
            *mode = DashboardMode::AddMethod;
            Ok(InputResult::Continue)
        }
        'd' => {
            if let Some(account) = accounts.get(*selected) {
                handle_delete_confirmation(account, stdout)
            } else {
                Ok(InputResult::Continue)
            }
        }
        'e' => {
            if let Some(account) = accounts.get(*selected) {
                handle_export_qr(account, stdout)
            } else {
                Ok(InputResult::Continue)
            }
        }
        _ => Ok(InputResult::Continue),
    }
}

fn handle_search_mode_char(
    c: char,
    query: &mut String,
    selected: &mut usize,
) -> Result<InputResult, AppError> {
    query.push(c);
    *selected = 0;
    Ok(InputResult::Continue)
}

fn handle_add_mode_char(c: char, name_buffer: &mut String) -> Result<InputResult, AppError> {
    name_buffer.push(c);
    Ok(InputResult::Continue)
}

fn handle_add_method_mode_char(
    c: char,
    mode: &mut DashboardMode,
    stdout: &mut io::Stdout,
    name_buffer: &mut String,
) -> Result<InputResult, AppError> {
    match c.to_ascii_lowercase() {
        's' if cfg!(target_os = "macos") => handle_screenshot_add(stdout),
        'm' => {
            *mode = DashboardMode::Add;
            name_buffer.clear();
            Ok(InputResult::Continue)
        }
        _ => Ok(InputResult::Continue),
    }
}

fn setup_terminal_for_input(stdout: &mut io::Stdout) -> Result<(), AppError> {
    queue!(stdout, Clear(ClearType::All), MoveTo(0, 0), Show)?;
    stdout.flush()?;
    disable_raw_mode()?;
    Ok(())
}

fn restore_dashboard_state(stdout: &mut io::Stdout) -> Result<(), AppError> {
    enable_raw_mode()?;
    queue!(stdout, Clear(ClearType::All), Hide)?;
    stdout.flush()?;
    Ok(())
}

fn handle_add_mode(stdout: &mut io::Stdout, name: &str) -> Result<InputResult, AppError> {
    setup_terminal_for_input(stdout)?;

    if let Ok(secret) = prompt_password("Enter the Base32 secret: ") {
        if let Ok(()) = save_account(name, &secret) {
            queue!(stdout, Print(format!("Added account: {}", name)))?;
        }
    }

    restore_dashboard_state(stdout)?;

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
    copied_state: &mut CopiedState,
) -> Result<(), AppError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    if let Ok(code) = generate_totp(account, duration) {
        if let Ok(mut clipboard) = Clipboard::new() {
            let _ = clipboard.set_text(format!("{}", code));
            copied_state.mark_copied(&account.name);
        }
    }
    Ok(())
}

fn handle_export_qr(
    account: &crate::totp::Account,
    stdout: &mut io::Stdout,
) -> Result<InputResult, AppError> {
    use qrcode::{QrCode, render::unicode};

    setup_terminal_for_input(stdout)?;

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

    restore_dashboard_state(stdout)?;

    Ok(InputResult::Continue)
}

#[cfg(target_os = "macos")]
fn handle_screenshot_add(stdout: &mut io::Stdout) -> Result<InputResult, AppError> {
    use std::process::Command;
    use std::fs;

    setup_terminal_for_input(stdout)?;

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
        
        restore_dashboard_state(stdout)?;
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

    restore_dashboard_state(stdout)?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::totp::Account;
    use crate::Storage;
    use std::time::{Duration, SystemTime};

    fn create_test_account(name: &str) -> Account {
        Account {
            name: name.to_string(),
            secret: "JBSWY3DPEHPK3PXP".to_string(),
            issuer: "Test".to_string(),
            algorithm: "SHA1".to_string(),
            digits: 6,
            period: 30,
            epoch: 0,
        }
    }

    fn create_test_storage() -> Storage {
        Storage {
            accounts: vec![
                create_test_account("GitHub"),
                create_test_account("Google"),
                create_test_account("Amazon"),
                create_test_account("Microsoft"),
            ],
        }
    }

    #[test]
    fn test_screen_buffer_creation() {
        let buffer = ScreenBuffer::new(80, 24);
        assert_eq!(buffer.width, 80);
        assert_eq!(buffer.height, 24);
        assert_eq!(buffer.lines.len(), 24);
        
        for line in &buffer.lines {
            assert!(line.content.is_empty());
            assert!(!line.is_highlighted);
            assert!(line.copied_split_pos.is_none());
        }
    }

    #[test]
    fn test_screen_buffer_write_operations() {
        let mut buffer = ScreenBuffer::new(80, 24);
        
        buffer.write_line(0, "Header".to_string());
        buffer.write_highlighted_line(1, "Selected".to_string());
        buffer.write_highlighted_line_with_copied(2, "Copied Item".to_string(), 6);
        
        assert_eq!(buffer.lines[0].content, "Header");
        assert!(!buffer.lines[0].is_highlighted);
        assert!(buffer.lines[0].copied_split_pos.is_none());
        
        assert_eq!(buffer.lines[1].content, "Selected");
        assert!(buffer.lines[1].is_highlighted);
        assert!(buffer.lines[1].copied_split_pos.is_none());
        
        assert_eq!(buffer.lines[2].content, "Copied Item");
        assert!(buffer.lines[2].is_highlighted);
        assert_eq!(buffer.lines[2].copied_split_pos, Some(6));
    }

    #[test]
    fn test_screen_buffer_bounds_checking() {
        let mut buffer = ScreenBuffer::new(80, 5);
        
        // Write within bounds
        buffer.write_line(4, "Valid".to_string());
        assert_eq!(buffer.lines[4].content, "Valid");
        
        // Write out of bounds - should not panic or affect buffer
        buffer.write_line(10, "Out of bounds".to_string());
        assert_eq!(buffer.lines[4].content, "Valid"); // Last line unchanged
    }

    #[test]
    fn test_screen_buffer_clear() {
        let mut buffer = ScreenBuffer::new(80, 24);
        
        buffer.write_highlighted_line_with_copied(0, "Test content".to_string(), 4);
        buffer.clear();
        
        for line in &buffer.lines {
            assert!(line.content.is_empty());
            assert!(!line.is_highlighted);
            assert!(line.copied_split_pos.is_none());
        }
    }

    #[test]
    fn test_copied_state_tracking() {
        let mut copied_state = CopiedState::new();
        
        // Initially no accounts are copied
        assert!(!copied_state.is_recently_copied("GitHub"));
        
        // Mark account as copied
        copied_state.mark_copied("GitHub");
        assert!(copied_state.is_recently_copied("GitHub"));
        assert!(!copied_state.is_recently_copied("Google"));
        
        // Multiple accounts
        copied_state.mark_copied("Google");
        assert!(copied_state.is_recently_copied("GitHub"));
        assert!(copied_state.is_recently_copied("Google"));
    }

    #[test]
    fn test_copied_state_cleanup() {
        let mut copied_state = CopiedState::new();
        
        // Manually insert old entry
        let old_time = SystemTime::now() - Duration::from_secs(3);
        copied_state.accounts.insert("OldAccount".to_string(), old_time);
        
        // Add recent entry
        copied_state.mark_copied("NewAccount");
        
        // Before cleanup
        assert_eq!(copied_state.accounts.len(), 2);
        
        // After cleanup
        copied_state.cleanup_old_entries();
        assert_eq!(copied_state.accounts.len(), 1);
        assert!(copied_state.accounts.contains_key("NewAccount"));
        assert!(!copied_state.accounts.contains_key("OldAccount"));
    }

    #[test]
    fn test_get_filtered_accounts_list_mode() {
        let storage = create_test_storage();
        let mode = DashboardMode::List;
        let matcher = SkimMatcherV2::default();
        
        let filtered = get_filtered_accounts(&storage, &mode, &matcher);
        
        assert_eq!(filtered.len(), 4);
        assert_eq!(filtered[0].name, "GitHub");
        assert_eq!(filtered[1].name, "Google");
        assert_eq!(filtered[2].name, "Amazon");
        assert_eq!(filtered[3].name, "Microsoft");
    }

    #[test]
    fn test_get_filtered_accounts_search_mode() {
        let storage = create_test_storage();
        let mode = DashboardMode::Search("Git".to_string());
        let matcher = SkimMatcherV2::default();
        
        let filtered = get_filtered_accounts(&storage, &mode, &matcher);
        
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "GitHub");
    }

    #[test]
    fn test_get_filtered_accounts_search_mode_no_matches() {
        let storage = create_test_storage();
        let mode = DashboardMode::Search("NonExistent".to_string());
        let matcher = SkimMatcherV2::default();
        
        let filtered = get_filtered_accounts(&storage, &mode, &matcher);
        
        assert_eq!(filtered.len(), 0);
    }

    #[test]
    fn test_get_filtered_accounts_search_mode_multiple_matches() {
        let storage = create_test_storage();
        let mode = DashboardMode::Search("o".to_string()); // Matches Google, Amazon, Microsoft
        let matcher = SkimMatcherV2::default();
        
        let filtered = get_filtered_accounts(&storage, &mode, &matcher);
        
        assert_eq!(filtered.len(), 3);
        // Results should be sorted by fuzzy match score
        let names: Vec<&String> = filtered.iter().map(|a| &a.name).collect();
        assert!(names.contains(&&"Google".to_string()));
        assert!(names.contains(&&"Amazon".to_string()));
        assert!(names.contains(&&"Microsoft".to_string()));
    }

    #[test]
    fn test_get_filtered_accounts_add_modes() {
        let storage = create_test_storage();
        let matcher = SkimMatcherV2::default();
        
        // Test Add mode
        let mode = DashboardMode::Add;
        let filtered = get_filtered_accounts(&storage, &mode, &matcher);
        assert_eq!(filtered.len(), 4);
        
        // Test AddMethod mode
        let mode = DashboardMode::AddMethod;
        let filtered = get_filtered_accounts(&storage, &mode, &matcher);
        assert_eq!(filtered.len(), 4);
    }

    #[test]
    fn test_handle_search_mode_char() {
        let mut query = String::new();
        let mut selected = 5;
        
        let result = handle_search_mode_char('a', &mut query, &mut selected);
        
        assert!(result.is_ok());
        assert_eq!(query, "a");
        assert_eq!(selected, 0); // Should reset selection
        
        // Add another character
        let result = handle_search_mode_char('b', &mut query, &mut selected);
        assert!(result.is_ok());
        assert_eq!(query, "ab");
        assert_eq!(selected, 0);
    }

    #[test]
    fn test_handle_add_mode_char() {
        let mut name_buffer = String::new();
        
        let result = handle_add_mode_char('G', &mut name_buffer);
        
        assert!(result.is_ok());
        assert_eq!(name_buffer, "G");
        
        // Add more characters
        let result = handle_add_mode_char('i', &mut name_buffer);
        assert!(result.is_ok());
        assert_eq!(name_buffer, "Gi");
    }

    #[test]
    fn test_extract_account_from_otpauth_valid() {
        let uri = "otpauth://totp/GitHub?secret=ABC123&issuer=GitHub";
        let result = extract_account_from_otpauth(uri);
        assert_eq!(result, Some("GitHub".to_string()));
    }

    #[test]
    fn test_extract_account_from_otpauth_with_encoded_name() {
        let uri = "otpauth://totp/My%20Account?secret=ABC123";
        let result = extract_account_from_otpauth(uri);
        assert_eq!(result, Some("My Account".to_string()));
    }

    #[test]
    fn test_extract_account_from_otpauth_no_query() {
        let uri = "otpauth://totp/SimpleAccount";
        let result = extract_account_from_otpauth(uri);
        assert_eq!(result, Some("SimpleAccount".to_string()));
    }

    #[test]
    fn test_extract_account_from_otpauth_invalid() {
        let uri = "invalid://uri";
        let result = extract_account_from_otpauth(uri);
        assert_eq!(result, None);
        
        let uri = "otpauth://hotp/Account"; // Wrong type
        let result = extract_account_from_otpauth(uri);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_secret_from_otpauth_valid() {
        let uri = "otpauth://totp/GitHub?secret=ABC123&issuer=GitHub";
        let result = extract_secret_from_otpauth(uri);
        assert_eq!(result, Some("ABC123".to_string()));
    }

    #[test]
    fn test_extract_secret_from_otpauth_no_secret() {
        let uri = "otpauth://totp/GitHub?issuer=GitHub";
        let result = extract_secret_from_otpauth(uri);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_secret_from_otpauth_invalid_uri() {
        let uri = "invalid://uri";
        let result = extract_secret_from_otpauth(uri);
        assert_eq!(result, None);
    }

    #[test]
    fn test_dashboard_mode_display() {
        // Test that modes can be created and compared
        let list_mode = DashboardMode::List;
        let search_mode = DashboardMode::Search("test".to_string());
        let add_mode = DashboardMode::Add;
        let add_method_mode = DashboardMode::AddMethod;
        
        assert!(matches!(list_mode, DashboardMode::List));
        assert!(matches!(search_mode, DashboardMode::Search(_)));
        assert!(matches!(add_mode, DashboardMode::Add));
        assert!(matches!(add_method_mode, DashboardMode::AddMethod));
        
        if let DashboardMode::Search(query) = search_mode {
            assert_eq!(query, "test");
        }
    }
}
