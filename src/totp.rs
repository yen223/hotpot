use base32::{Alphabet, decode};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Sha256, Sha512};
use std::time::Duration;

use super::AppError;

#[derive(Serialize, Deserialize, Clone)]
pub struct Account {
    pub name: String,
    /// Base32 encoded secret key (RFC4648 without padding)
    pub secret: String,
    #[serde(default = "default_issuer")]
    pub issuer: String,
    #[serde(default = "default_algorithm")]
    pub algorithm: String,
    #[serde(default = "default_digits")]
    pub digits: u32,
    #[serde(default = "default_period")]
    pub period: u32,
    #[serde(default = "default_epoch")]
    pub epoch: u64,
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

fn default_epoch() -> u64 {
    0 // Default epoch is Unix epoch (1970-01-01 00:00:00 UTC)
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
            epoch: default_epoch(),
        }
    }

    pub fn generate_uri(&self) -> String {
        let label = format!("{}:{}", self.issuer, self.name);
        let digits = self.digits.to_string();
        let period = self.period.to_string();
        let params = [
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

pub fn generate_totp(account: &Account, duration: Duration) -> Result<u32, AppError> {
    let secret_bytes = match decode(Alphabet::RFC4648 { padding: false }, &account.secret) {
        Some(bytes) => bytes,
        None => return Err(AppError::new("Bytes could not be decoded")),
    };

    // T = (Current Unix time - T0) / X, where:
    // - Current Unix time = duration.as_secs()
    // - T0 = account.epoch (default 0 for Unix epoch)
    // - X = account.period (default 30 seconds)
    let counter = (duration.as_secs().saturating_sub(account.epoch)) / u64::from(account.period);

    // Convert counter to exactly 8 bytes big-endian per RFC 6238
    let counter_bytes = counter.to_be_bytes();

    let result = match account.algorithm.as_str() {
        "SHA1" => {
            let mut mac =
                Hmac::<Sha1>::new_from_slice(&secret_bytes).expect("HMAC can take key of any size");
            mac.update(&counter_bytes);
            mac.finalize().into_bytes().to_vec()
        }
        "SHA256" => {
            let mut mac = Hmac::<Sha256>::new_from_slice(&secret_bytes)
                .expect("HMAC can take key of any size");
            mac.update(&counter_bytes);
            mac.finalize().into_bytes().to_vec()
        }
        "SHA512" => {
            let mut mac = Hmac::<Sha512>::new_from_slice(&secret_bytes)
                .expect("HMAC can take key of any size");
            mac.update(&counter_bytes);
            mac.finalize().into_bytes().to_vec()
        }
        _ => return Err(AppError::new("Unsupported algorithm")),
    };

    // Use last byte of hash to determine offset
    // Per RFC 6238, get offset from last byte and extract 4 bytes starting at that offset
    let offset = (result[result.len() - 1] & 0xf) as usize;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // Test vectors from RFC 6238
    // The test token shared secrets use ASCII strings with lengths matching each hash:
    const TEST_SECRET_SHA1: &str = "12345678901234567890"; // 20 bytes
    const TEST_SECRET_SHA256: &str = "12345678901234567890123456789012"; // 32 bytes
    const TEST_SECRET_SHA512: &str =
        "1234567890123456789012345678901234567890123456789012345678901234"; // 64 bytes

    struct TestVector {
        time: u64,
        expected_totp: u32,
        algorithm: &'static str,
        secret: &'static str,
    }

    fn ascii_to_base32(ascii: &str) -> String {
        base32::encode(Alphabet::RFC4648 { padding: false }, ascii.as_bytes())
    }

    fn create_test_account(secret: &str) -> Account {
        Account {
            name: "test".to_string(),
            secret: ascii_to_base32(secret),
            issuer: default_issuer(),
            algorithm: default_algorithm(),
            digits: 8, // RFC test vectors use 8 digits
            period: 30,
            epoch: default_epoch(),
        }
    }

    #[test]
    fn test_rfc6238_vectors() {
        let test_vectors = vec![
            // SHA1 test vectors
            TestVector {
                time: 59,
                expected_totp: 94287082,
                algorithm: "SHA1",
                secret: TEST_SECRET_SHA1,
            },
            TestVector {
                time: 1111111109,
                expected_totp: 7081804,
                algorithm: "SHA1",
                secret: TEST_SECRET_SHA1,
            },
            TestVector {
                time: 1111111111,
                expected_totp: 14050471,
                algorithm: "SHA1",
                secret: TEST_SECRET_SHA1,
            },
            TestVector {
                time: 1234567890,
                expected_totp: 89005924,
                algorithm: "SHA1",
                secret: TEST_SECRET_SHA1,
            },
            TestVector {
                time: 2000000000,
                expected_totp: 69279037,
                algorithm: "SHA1",
                secret: TEST_SECRET_SHA1,
            },
            TestVector {
                time: 20000000000,
                expected_totp: 65353130,
                algorithm: "SHA1",
                secret: TEST_SECRET_SHA1,
            },
            // SHA256 test vectors
            TestVector {
                time: 59,
                expected_totp: 46119246,
                algorithm: "SHA256",
                secret: TEST_SECRET_SHA256,
            },
            TestVector {
                time: 1111111109,
                expected_totp: 68084774,
                algorithm: "SHA256",
                secret: TEST_SECRET_SHA256,
            },
            TestVector {
                time: 1111111111,
                expected_totp: 67062674,
                algorithm: "SHA256",
                secret: TEST_SECRET_SHA256,
            },
            TestVector {
                time: 1234567890,
                expected_totp: 91819424,
                algorithm: "SHA256",
                secret: TEST_SECRET_SHA256,
            },
            TestVector {
                time: 2000000000,
                expected_totp: 90698825,
                algorithm: "SHA256",
                secret: TEST_SECRET_SHA256,
            },
            TestVector {
                time: 20000000000,
                expected_totp: 77737706,
                algorithm: "SHA256",
                secret: TEST_SECRET_SHA256,
            },
            // SHA512 test vectors
            TestVector {
                time: 59,
                expected_totp: 90693936,
                algorithm: "SHA512",
                secret: TEST_SECRET_SHA512,
            },
            TestVector {
                time: 1111111109,
                expected_totp: 25091201,
                algorithm: "SHA512",
                secret: TEST_SECRET_SHA512,
            },
            TestVector {
                time: 1111111111,
                expected_totp: 99943326,
                algorithm: "SHA512",
                secret: TEST_SECRET_SHA512,
            },
            TestVector {
                time: 1234567890,
                expected_totp: 93441116,
                algorithm: "SHA512",
                secret: TEST_SECRET_SHA512,
            },
            TestVector {
                time: 2000000000,
                expected_totp: 38618901,
                algorithm: "SHA512",
                secret: TEST_SECRET_SHA512,
            },
            TestVector {
                time: 20000000000,
                expected_totp: 47863826,
                algorithm: "SHA512",
                secret: TEST_SECRET_SHA512,
            },
        ];

        for vector in test_vectors {
            let mut account = create_test_account(vector.secret);
            account.algorithm = vector.algorithm.to_string();
            let duration = Duration::from_secs(vector.time);
            let result = generate_totp(&account, duration).unwrap();
            assert_eq!(
                result, vector.expected_totp,
                "Failed at timestamp {} with {}: got {} but expected {}",
                vector.time, vector.algorithm, result, vector.expected_totp
            );
        }
    }

    #[test]
    fn test_invalid_secret() {
        let mut account = create_test_account(TEST_SECRET_SHA1);
        account.secret = "invalid base32".to_string();

        let duration = Duration::from_secs(59);
        assert!(generate_totp(&account, duration).is_err());
    }

    #[test]
    fn test_invalid_algorithm() {
        let mut account = create_test_account(TEST_SECRET_SHA1);
        account.algorithm = "SHA999".to_string();

        let duration = Duration::from_secs(59);
        assert!(generate_totp(&account, duration).is_err());
    }

    #[test]
    fn test_custom_epoch() {
        let mut account = create_test_account(TEST_SECRET_SHA1);
        account.epoch = 1111111109; // One of the RFC test times
        account.algorithm = "SHA1".to_string();

        // Test using a duration that's 30 seconds after our epoch
        let duration = Duration::from_secs(1111111139);
        let result = generate_totp(&account, duration).unwrap();

        // Should give same result as RFC test vector for time=59
        // because (1111111139 - 1111111109) / 30 = 1
        // which is same as (59 - 0) / 30 = 1
        assert_eq!(result, 94287082);
    }
}
