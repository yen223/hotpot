# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Hotpot is a command-line TOTP (Time-based One-Time Password) authenticator written in Rust. It securely stores 2FA secrets in the system keyring and provides both interactive dashboard and CLI interfaces for managing and generating TOTP codes.

## Development Commands

### Building and Installation
- `cargo build --release` - Build optimized release binary
- `cargo install --path . --locked` - Install locally from source
- `cargo run` - Run in development mode
- `cargo run -- <subcommand>` - Run with specific CLI arguments (e.g., `cargo run -- add github`)

### Testing and Quality
- `cargo test` - Run unit tests (if any exist)
- `cargo check` - Quick compile check without producing binary  
- `cargo clippy` - Run linter for code quality

## Architecture

### Core Components

**Main Entry Point (`src/main.rs:128-162`)**
- CLI argument parsing using clap
- Command routing and error handling
- Integration with keyring storage

**TOTP Implementation (`src/totp.rs`)**
- `Account` struct with configurable TOTP parameters (algorithm, digits, period)
- HMAC-based TOTP generation supporting SHA1/SHA256/SHA512
- otpauth URI generation for QR code export

**Interactive Dashboard (`src/dashboard.rs`)**
- Real-time TOTP code display with progress bars
- Fuzzy search functionality using skim matcher
- Keyboard navigation and clipboard integration
- Multiple modes: List, Search, Add

**Storage Layer (`src/main.rs:55-82`)**
- JSON serialization of account data
- Secure storage via system keyring (macOS Keychain, Linux Secret Service, Windows Credential Manager)
- Account management (add, delete, retrieve)

### Key Storage Pattern
All secrets are stored in the system keyring under service name "hotpot" with storage key "_hotpot_storage" as JSON. Accounts are sorted alphabetically and stored as a serialized `Storage` struct.

### Error Handling
Custom `AppError` type in `src/lib.rs` with conversions from keyring, JSON, and IO errors. All errors bubble up to main for consistent user-facing error messages.

## CLI Interface

The application supports both interactive mode (default) and specific commands:
- `hotpot` - Interactive dashboard
- `hotpot add <name>` - Add new account
- `hotpot code <name>` - Generate single code
- `hotpot delete <name>` - Delete account  
- `hotpot export-qr --name <name>` - Export QR code

## Dependencies

Key external crates:
- `keyring` - Secure credential storage
- `clap` - CLI argument parsing
- `crossterm` - Terminal UI and input handling
- `qrcode` - QR code generation for mobile app setup
- `fuzzy-matcher` - Interactive search functionality
- `arboard` - Clipboard operations