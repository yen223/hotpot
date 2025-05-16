# Hotpot 🍲

Authy / Google Authenticator for the command line.

Hotpot is a simple and secure command-line tool for managing TOTP-based two-factor authentication codes. Hotpot securely stores your 2FA secrets in your system's keyring and generates time-based one-time passwords when you need them.

This was largely vibe-coded, so use at your own risk!

## Features

- 🔒 Secure storage using system keyring
- 🕒 TOTP (RFC 6238) code generation
- 💻 Simple command-line interface
- 🗂️ Easy account management with fuzzy search

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

### Generate a code

```bash
hotpot code <account-name>
```

Example:
```bash
hotpot code github
```

### List and search accounts

```bash
hotpot list
```

Interactively search for accounts using fuzzy matching. Type to filter accounts in real-time, use up/down arrows to navigate, and press Enter to generate the code for the selected account. Press 'q', 'Esc', or Ctrl+C to exit.

### Delete an account

```bash
hotpot delete <account-name>
```

## Security

Hotpot stores all secrets securely in your system's keyring:
- macOS: Keychain
- Linux: Secret Service API/libsecret
- Windows: Windows Credential Manager

## Building from Source

1. Ensure you have Rust installed
2. Clone the repository
3. Run:
```bash
cargo build --release
```

The binary will be available at `target/release/hotpot`

## Dependencies

- clap: Command line argument parsing
- keyring: Secure secret storage
- base32: RFC 4648 base32 encoding/decoding
- hmac & sha1: TOTP algorithm implementation
- serde & serde_json: Data serialization
- rpassword: Secure password/secret input
- crossterm: Terminal manipulation and display
- fuzzy-matcher: Interactive fuzzy searching

## License

MIT License

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.
