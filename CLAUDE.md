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
- `cargo test` - Run comprehensive unit tests (25+ tests covering core functionality)
- `cargo check` - Quick compile check without producing binary  
- `cargo clippy` - Run linter for code quality

## Architecture

### Core Components

**Main Entry Point (`src/main.rs:128-162`)**
- CLI argument parsing using clap
- Command routing and error handling
- Integration with keyring storage

**TOTP Implementation (`src/totp.rs`)**
- `Account` struct with configurable TOTP parameters (algorithm, digits, period, epoch)
- HMAC-based TOTP generation supporting SHA1/SHA256/SHA512
- otpauth URI generation for QR code export
- Comprehensive test coverage including RFC 6238 test vectors

**Interactive Dashboard (`src/dashboard.rs`)**
- Real-time TOTP code display with smooth Unicode progress bars
- Fuzzy search functionality using skim matcher
- Keyboard navigation and clipboard integration with visual feedback
- Multiple modes: List, Search, Add, AddMethod
- Screenshot-based QR code import (macOS)
- Modular, well-tested architecture with 21 unit tests
- Double-buffered rendering system for smooth updates
- Terminal resize handling and responsive UI

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
- `hotpot` - Interactive dashboard with real-time updates
- `hotpot add <name>` - Add new account with secure secret input
- `hotpot code <name>` - Generate single code for account
- `hotpot delete <name>` - Delete account with confirmation  
- `hotpot export-qr --name <name>` - Export QR code to terminal
- `hotpot --load-image <path>` - Import account from QR code image

## Dependencies

Key external crates:
- `keyring` - Secure credential storage
- `clap` - CLI argument parsing
- `crossterm` - Terminal UI and input handling
- `qrcode` - QR code generation for mobile app setup
- `fuzzy-matcher` - Interactive search functionality
- `arboard` - Clipboard operations
- `image` + `rqrr` - QR code image processing and detection
- `url` + `urlencoding` - otpauth URI handling

## Code Quality & Testing

### Dashboard Module Tests (`src/dashboard.rs`)
The dashboard module includes comprehensive unit tests covering:

**ScreenBuffer Tests (5 tests):**
- Buffer creation and initialization
- Write operations (normal, highlighted, with copied indicator) 
- Bounds checking and safety
- Clear functionality

**State Management Tests (2 tests):**
- CopiedState tracking and cleanup
- Time-based entry expiration

**Business Logic Tests (8 tests):**
- Account filtering for List/Search/Add modes
- Fuzzy search matching and scoring
- Input handling for different modes
- Edge cases (no matches, invalid input)

**QR Code Parsing Tests (5 tests):**
- otpauth URI parsing and validation
- URL encoding/decoding support
- Invalid URI handling
- Secret extraction

**Mode Management Tests (1 test):**
- Dashboard mode creation and pattern matching

### TOTP Module Tests (`src/totp.rs`)
- RFC 6238 compliance test vectors
- Algorithm validation (SHA1/SHA256/SHA512)
- Invalid secret handling
- Custom epoch support

### Recent Refactoring Improvements
The codebase has been significantly refactored for maintainability:

1. **Extracted Helper Functions**: Complex rendering and input logic broken into focused functions
2. **Consolidated Terminal Operations**: Removed code duplication in terminal state management  
3. **Simplified Account Filtering**: Centralized filtering logic for all dashboard modes
4. **Unified Buffer Operations**: Single parameterized function for all line writing operations
5. **Reduced Variable Redundancy**: Eliminated unnecessary variable tracking

These improvements make the code more testable, maintainable, and easier to understand while preserving all functionality.