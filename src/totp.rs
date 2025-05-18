use base32::{decode, Alphabet};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use super::AppError;

#[derive(Serialize, Deserialize, Clone)]
pub struct Account {
    pub name: String,
    pub secret: String,
    #[serde(default = "default_issuer")]
    pub issuer: String,
    #[serde(default = "default_algorithm")]
    pub algorithm: String,
    #[serde(default = "default_digits")]
    pub digits: u32,
    #[serde(default = "default_period")]
    pub period: u32,
}

fn default_issuer() -> String {
    "hotpot".to_string()
}

fn default_algorithm() -> String {
    "SHA1".to_string()
}

fn default_digits() -> u32 {
    6
}

fn default_period() -> u32 {
    30
}

impl Account {
    pub fn new(name: String, secret: String) -> Self {
        Self {
            name,
            secret,
            issuer: default_issuer(),
            algorithm: default_algorithm(),
            digits: default_digits(),
            period: default_period(),
        }
    }

    pub fn generate_uri(&self) -> String {
        let label = format!("{}:{}", self.issuer, self.name);
        let digits = self.digits.to_string();
        let period = self.period.to_string();
        let params = vec![
            ("secret", &self.secret),
            ("issuer", &self.issuer),
            ("algorithm", &self.algorithm),
            ("digits", &digits),
            ("period", &period),
        ];

        let query = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");

        format!("otpauth://totp/{}?{}", label, query)
    }
}

pub fn generate_totp(account: &Account) -> Result<u32, AppError> {
    let secret_bytes = match decode(Alphabet::RFC4648 { padding: false }, &account.secret) {
        Some(bytes) => bytes,
        None => return Err(AppError::new("Bytes could not be decoded")),
    };

    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time is before Unix epoch");
    let counter = duration.as_secs() / u64::from(account.period);

    let mut mac = match account.algorithm.as_str() {
        "SHA1" => {
            Hmac::<Sha1>::new_from_slice(&secret_bytes).expect("HMAC can take key of any size")
        }
        _ => return Err(AppError::new("Unsupported algorithm")), // Add support for SHA256/SHA512 if needed
    };

    mac.update(&counter.to_be_bytes());
    let result = mac.finalize().into_bytes();

    let offset = (result[19] & 0xf) as usize;
    let binary = ((u32::from(result[offset]) & 0x7f) << 24)
        | ((u32::from(result[offset + 1]) & 0xff) << 16)
        | ((u32::from(result[offset + 2]) & 0xff) << 8)
        | (u32::from(result[offset + 3]) & 0xff);

    let modulus = 10u32.pow(account.digits);
    Ok(binary % modulus)
}

pub fn generate_otpauth_uri(name: &str, secret: &str) -> String {
    Account::new(name.to_string(), secret.to_string()).generate_uri()
}