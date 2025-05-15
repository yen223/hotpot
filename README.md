# Hotpot 🍲

A simple and secure command-line tool for managing TOTP-based two-factor authentication codes. Hotpot securely stores your 2FA secrets in your system's keyring and generates time-based one-time passwords when you need them.

This was largely vibe-coded, so use at your own risk!

## Features

- 🔒 Secure storage using system keyring
- 🕒 TOTP (RFC 6238) code generation
- 💻 Simple command-line interface
- 🗂️ Easy account management

## Installation

```bash
cargo build --release
```

## Usage

### Add a new account

```bash
hotpot add <account-name> <base32-secret>
```

Example:
```bash
hotpot add github JBSWY3DPEHPK3PXP
```

### Generate a code

```bash
hotpot code <account-name>
```

Example:
```bash
hotpot code github
```

### List all accounts

```bash
hotpot list
```

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

## License

MIT License

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.
