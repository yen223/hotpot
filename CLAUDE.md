# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Hotpot is a command-line TOTP (Time-based One-Time Password) authenticator written in Rust. It securely stores 2FA secrets in the system keyring by default, with an optional file-backed storage mode for portable configurations. It provides both interactive dashboard and CLI interfaces for managing and generating TOTP codes.

## Development Commands

### Building and Installation
- `cargo build --release` - Build optimized release binary
- `cargo install --path . --locked` - Install locally from source
- `cargo run` - Run in development mode
- `cargo run -- <subcommand>` - Run with specific CLI arguments (e.g., `cargo run -- add github`)

### Testing and Quality
- `cargo test` - Run all tests (48 total: 25 unit tests + 4 TOTP tests + 19 integration tests)
- `cargo test --test lib` - Run integration tests only
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

**Storage Layer (`src/main.rs:68-133`)**
- JSON serialization of account data
- Dual storage backends: secure keyring (default) and file-backed (optional)
- Secure storage via system keyring (macOS Keychain, Linux Secret Service, Windows Credential Manager)
- File-backed storage with automatic directory creation and proper permissions
- Account management (add, delete, retrieve) with backend selection

### Key Storage Pattern
**Keyring Storage (Default):** All secrets are stored in the system keyring under service name "hotpot" with storage key "_hotpot_storage" as JSON. Accounts are sorted alphabetically and stored as a serialized `Storage` struct.

**File-Backed Storage (Optional):** When using the `--file` flag, accounts are stored in a JSON file at the specified path. The file is created with proper permissions (600) and parent directories are created automatically if needed. This mode is ideal for portable configurations, server environments, or when keyring access is unavailable.

**File Format Example:**
```json
{
  "accounts": [
    {
      "name": "github",
      "secret": "JBSWY3DPEHPK3PXP",
      "issuer": "",
      "algorithm": "SHA1",
      "digits": 6,
      "period": 30,
      "epoch": 0
    }
  ]
}
```

### Error Handling
Custom `AppError` type in `src/lib.rs` with conversions from keyring, JSON, and IO errors. All errors bubble up to main for consistent user-facing error messages.

### Known Issues & Limitations

**rpassword Environment Compatibility:**
The `rpassword` crate (used for secure password input) can fail with "Device not configured (os error 6)" in certain environments:
- When stdin is redirected (e.g., `echo "secret" | hotpot add account`)
- In some CI/CD environments or containers
- On macOS in certain terminal configurations
- When TTY access is restricted

This affects the `add` command's password prompting but does not impact:
- Non-interactive commands (`code`, `delete`, `export-qr`)
- Dashboard mode (uses different input handling)
- File storage functionality (the core feature works correctly)

**Workarounds:**
- Use the interactive dashboard for adding accounts (press 'A' then 'M')
- Pre-create JSON files with account data for testing
- Consider alternative password input methods for automated environments

## CLI Interface

The application supports both interactive mode (default) and specific commands:
- `hotpot` - Interactive dashboard with real-time updates
- `hotpot add <name>` - Add new account with secure secret input
- `hotpot code <name>` - Generate single code for account
- `hotpot delete <name>` - Delete account with confirmation  
- `hotpot export-qr --name <name>` - Export QR code to terminal
- `hotpot add --image <path>` - Import account from QR code image (optionally specify account name)
- `hotpot add <name> --image <path>` - Import account from QR code image with specific name

**File-Backed Storage Mode:**
Add the `--file` flag to any command to use file-backed storage instead of the secure keyring:
- `hotpot --file accounts.json` - Interactive dashboard with file storage
- `hotpot --file accounts.json add <name>` - Add account to file
- `hotpot --file accounts.json code <name>` - Generate code from file-stored account
- `hotpot --file accounts.json delete <name>` - Delete account from file

This mode is perfect for portable configurations, server environments, or when keyring services are unavailable.


## Dependencies

**Runtime Dependencies:**
- `keyring` - Secure credential storage
- `clap` - CLI argument parsing
- `crossterm` - Terminal UI and input handling
- `qrcode` - QR code generation for mobile app setup
- `fuzzy-matcher` - Interactive search functionality
- `arboard` - Clipboard operations
- `image` + `rqrr` - QR code image processing and detection
- `url` + `urlencoding` - otpauth URI handling

**Development Dependencies:**
- `tempfile` - Temporary file management for integration tests

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

### Integration Test Suite (`tests/`)
The project includes comprehensive integration tests that validate end-to-end CLI functionality using file-backed storage to avoid keychain dependencies.

**Test Structure:**
```
tests/
├── integration/
│   ├── mod.rs              # Test utilities and TestContext
│   ├── cli_commands.rs     # CLI command integration tests (11 tests)
│   ├── file_storage.rs     # File storage backend tests (8 tests)
│   └── fixtures/           # Test data files with known TOTP secrets
└── lib.rs                  # Integration test entry point
```

**CLI Command Tests (11 tests):**
- TOTP code generation for existing/nonexistent accounts
- Account deletion with confirmation handling  
- QR code export functionality
- Multiple accounts in same file operations
- Invalid JSON file error handling
- File creation for new storage locations

**File Storage Backend Tests (8 tests):**
- File permissions (600) and parent directory auto-creation
- Empty file and malformed JSON error handling
- Missing required fields validation
- Concurrent file access safety
- Large file handling (100+ accounts)
- Special characters in account names
- File persistence across multiple operations

**Test Infrastructure:**
- `TestContext` - Manages temporary files with automatic cleanup
- Pre-populated fixtures with RFC 6238 compliant test data
- Command execution helpers with stdin simulation
- Account validation and JSON parsing utilities

**Key Benefits:**
- **Keychain-free**: No system keyring pollution during testing
- **CI/CD ready**: Works in any environment without external dependencies
- **Isolated**: Each test uses temporary files for complete isolation
- **Fast**: No network calls or system service dependencies
- **Comprehensive**: Covers all CLI commands, storage backends, and edge cases

**Running Integration Tests:**
- `cargo test --test lib` - Run only integration tests
- `cargo test integration::cli_commands` - Run CLI command tests only  
- `cargo test integration::file_storage` - Run file storage tests only

The integration test suite validates that the `--file` storage mode provides identical functionality to keyring storage while being completely portable and testable.