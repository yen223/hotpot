# Hotpot 🍲

Authy / Google Authenticator for the command line.

Hotpot is a simple and secure command-line tool for managing TOTP-based two-factor authentication codes. Hotpot securely stores your 2FA secrets in your system's keyring and generates time-based one-time passwords when you need them.

Built with Rust for security, performance, and reliability.

## Features

- 🔒 **Secure storage** using system keyring (Keychain/libsecret/Credential Manager)
- 📁 **File-backed storage** option for portable configurations (optional `--file` flag)
- 🕒 **TOTP (RFC 6238)** code generation with customizable algorithms (SHA1/SHA256/SHA512)
- 💻 **Interactive dashboard** with real-time codes, progress bars, and fuzzy search
- 📋 **One-click copy** to clipboard with visual feedback
- 📱 **QR code export** for easy mobile app setup
- 🖥️ **Screenshot capture** (macOS) for importing QR codes
- ⚡ **Fast and responsive** terminal UI with smooth animations
- 🔍 **Fuzzy search** to quickly find accounts
- 🧪 **Comprehensive test coverage** for reliability

## Installation

```bash
cargo install --path . --locked
```

## Usage

### Add a new account

```bash
hotpot add <account-name>
```

You will be prompted to enter the Base32 secret securely.

Example:
```bash
hotpot add github
Enter the Base32 secret: ********
```

#### Add from QR code image (macOS)

```bash
hotpot --load-image /path/to/qr-code.png
```

Or use the interactive screenshot capture in the dashboard by pressing [A] then [S].

### Interactive Dashboard

Just run `hotpot` to open the interactive dashboard where you can:

- **View all TOTP codes** in real-time with smooth progress bars
- **Navigate** with up/down arrows
- **Copy codes** by pressing Enter (shows "copied" indicator)
- **Search** by pressing [F] and typing (fuzzy matching)
- **Add accounts** by pressing [A], then choose [M]anual or [S]creenshot (macOS)
- **Delete accounts** by pressing [D] (with confirmation)
- **Export QR codes** by pressing [E] for mobile app setup
- **Exit** with 'q', 'Esc', or Ctrl+C

The dashboard automatically refreshes every 250ms and handles terminal resizing gracefully.

### Generate a single code

```bash
hotpot code <account-name>
```

Example:
```bash
hotpot code github
```

### Delete an account

```bash
hotpot delete <account-name>
```

### Export QR Code

You can export a QR code for an account either through the dashboard (press [E]) or using the command:

```bash
hotpot export-qr --name <account-name>
```

This will display a QR code in the terminal that can be scanned by authenticator apps.

### File-Backed Storage Mode

For portable configurations or when keyring access is unavailable, you can use the `--file` flag to store accounts in a JSON file instead of the secure keyring.

```bash
# Interactive dashboard with file storage
hotpot --file ~/.config/hotpot/accounts.json

# Add account to file
hotpot --file ./my-accounts.json add work-account

# Generate code from file-stored account
hotpot --file ./my-accounts.json code work-account

# Delete account from file
hotpot --file ./my-accounts.json delete work-account
```

**Use cases for file-backed storage:**
- 📁 **Portable configurations**: Store accounts in a file that can be synced or backed up
- 🖥️ **Server environments**: Use when keyring services are unavailable
- 🔄 **Development/Testing**: Isolate test accounts from secure storage
- 📋 **Team sharing**: Share account configurations (ensure file security)

**Security Note**: File-backed storage stores secrets in plaintext JSON. Ensure proper file permissions (600) and consider encrypting the file for sensitive environments.


## Security

**Default Secure Storage:** Hotpot stores all secrets securely in your system's keyring:
- macOS: Keychain
- Linux: Secret Service API/libsecret
- Windows: Windows Credential Manager

**File-Backed Storage:** When using the `--file` flag, accounts are stored in a JSON file at the specified path. The file is created with appropriate permissions (600) and directories are created automatically if needed. This mode is useful for portable configurations or when keyring access is unavailable.


## Development

### Building from Source

1. Ensure you have Rust installed
2. Clone the repository
3. Run:
```bash
cargo build --release
```

The binary will be available at `target/release/hotpot`

### Running Tests

The project includes comprehensive unit tests covering core functionality:

```bash
cargo test
```

Test coverage includes:
- TOTP code generation and validation
- Account filtering and search logic
- QR code parsing and validation
- Dashboard state management
- Input handling and mode switching

### Development Commands

```bash
# Run in development mode
cargo run

# Run with specific arguments
cargo run -- add github

# Test with file storage
cargo run -- --file test-accounts.json add test-account


# Check code quality
cargo clippy

# Quick compile check
cargo check
```

## Architecture

Hotpot is built with a modular architecture focused on security and maintainability:

- **`main.rs`**: CLI interface and storage management
- **`totp.rs`**: TOTP algorithm implementation (RFC 6238) with comprehensive test coverage
- **`dashboard.rs`**: Interactive terminal UI with real-time updates and extensive unit tests
- **`lib.rs`**: Common error handling and shared utilities

### Key Dependencies

- **Security & Storage**: `keyring`, `base32`, `hmac`, `sha1/sha2`
- **CLI & Terminal**: `clap`, `crossterm`, `rpassword`
- **Interactive Features**: `fuzzy-matcher`, `qrcode`, `arboard`
- **Data Handling**: `serde`, `serde_json`, `url`, `urlencoding`
- **Image Processing**: `image`, `rqrr` (QR code detection)

## License

MIT License

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

### Code Quality

The project maintains high code quality standards:
- Comprehensive unit tests (run `cargo test`)
- Clippy linting (run `cargo clippy`)
- Well-documented functions and modules
- Secure coding practices with proper error handling

### Recent Improvements

- **Enhanced Dashboard**: Refactored UI code for better maintainability and performance
- **Comprehensive Testing**: Added 25+ unit tests covering core functionality
- **Improved UX**: Added visual feedback for copied codes and smooth progress bars
- **Code Quality**: Extracted helper functions and reduced code duplication
