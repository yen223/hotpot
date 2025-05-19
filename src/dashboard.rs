use std::{
    cmp::min,
    io::{self, Write},
    time::{SystemTime, UNIX_EPOCH},
};

use crossterm::{
    cursor::{Hide, MoveTo, MoveToNextLine, Show},
    event::{Event, KeyCode, KeyEvent, KeyModifiers, poll, read},
    execute,
    style::{Attribute, Color, SetAttribute, SetForegroundColor},
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode, size},
};
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

use crate::{AppError, get_storage, totp::generate_totp};

pub fn show() -> Result<(), AppError> {
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
    let mut search_mode = false;

    loop {
        // Get terminal size and calculate display area
        let (term_width, term_height) = size()?;
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
        if search_mode {
            print!("Search: {}_", query);
        } else {
            print!("[F]ind\t[A]dd account\t[D]elete");
        }
        execute!(stdout, MoveToNextLine(1))?;

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
            execute!(
                stdout,
                MoveTo(0, i as u16 + 2),
                Clear(ClearType::CurrentLine)
            )?;
            let code = generate_totp(account, now);
            let max_width = min(term_width, 64);
            let max_name_len = (max_width as usize).saturating_sub(10); // Leave room for code
            let display_name = if account.name.len() > max_name_len {
                format!("{}...", &account.name[..max_name_len.saturating_sub(3)])
            } else {
                account.name.clone()
            };

            if i == selected {
                execute!(stdout, SetAttribute(Attribute::Bold))?;
                print!("> {} ", &display_name);
                if let Ok(code) = code {
                    execute!(stdout, MoveTo(max_width.saturating_sub(7), i as u16 + 2))?;
                    print!("{:0width$}", code, width = account.digits as usize);
                }
                execute!(stdout, SetAttribute(Attribute::Reset))?;
            } else {
                print!("  {} ", &display_name);
                if let Ok(code) = code {
                    execute!(stdout, MoveTo(max_width.saturating_sub(7), i as u16 + 2))?;
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
                    if c == 'f' && !search_mode {
                        search_mode = true;
                    } else if search_mode {
                        query.push(c);
                        selected = 0;
                    }
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Backspace,
                    ..
                }) => {
                    if search_mode {
                        query.pop();
                        selected = 0;
                    }
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
                        let duration = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .expect("System time is before Unix epoch");
                        if let Ok(code) = generate_totp(account, duration) {
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
