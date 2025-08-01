use super::{TestContext, get_account_count, run_hotpot_command};
use std::fs;
use std::os::unix::fs::PermissionsExt;

#[test]
fn test_file_creation_with_proper_permissions() {
    let ctx = TestContext::new();

    // Create a file by running any command that would create it
    let _output = run_hotpot_command(&[
        "--file",
        ctx.file_path().to_str().unwrap(),
        "code",
        "nonexistent", // This will fail but should create the file
    ]);

    if ctx.file_path().exists() {
        let metadata = fs::metadata(ctx.file_path()).expect("Failed to get file metadata");
        let permissions = metadata.permissions();

        // On Unix systems, check that file permissions are restrictive (600)
        #[cfg(unix)]
        {
            let mode = permissions.mode();
            // Check that only owner has read/write permissions (0o600)
            assert_eq!(mode & 0o777, 0o600, "File should have 600 permissions");
        }
    }
}

#[test]
fn test_parent_directory_creation() {
    let temp_dir = tempfile::TempDir::new().expect("Failed to create temp directory");
    let nested_path = temp_dir
        .path()
        .join("nested")
        .join("deep")
        .join("accounts.json");

    let _output = run_hotpot_command(&[
        "--file",
        nested_path.to_str().unwrap(),
        "code",
        "nonexistent",
    ]);

    // Check that parent directories were created
    assert!(
        nested_path.parent().unwrap().exists(),
        "Parent directories should be created"
    );
}

#[test]
fn test_empty_file_handling() {
    let ctx = TestContext::with_empty_file();

    let output = run_hotpot_command(&[
        "--file",
        ctx.file_path().to_str().unwrap(),
        "code",
        "github",
    ]);

    assert!(
        !output.status.success(),
        "Command should fail with empty account list"
    );
}

#[test]
fn test_file_with_malformed_json() {
    let ctx = TestContext::new();
    fs::write(ctx.file_path(), r#"{"github": {"secret": "incomplete"#)
        .expect("Failed to write malformed JSON");

    let output = run_hotpot_command(&[
        "--file",
        ctx.file_path().to_str().unwrap(),
        "code",
        "github",
    ]);

    assert!(
        !output.status.success(),
        "Command should fail with malformed JSON"
    );
}

#[test]
fn test_file_with_missing_required_fields() {
    let ctx = TestContext::new();
    let invalid_data = r#"{
  "accounts": [
    {
      "name": "github",
      "algorithm": "SHA1",
      "digits": 6,
      "period": 30,
      "epoch": 0
    }
  ]
}"#;
    fs::write(ctx.file_path(), invalid_data).expect("Failed to write invalid data");

    let output = run_hotpot_command(&[
        "--file",
        ctx.file_path().to_str().unwrap(),
        "code",
        "github",
    ]);

    assert!(
        !output.status.success(),
        "Command should fail when secret is missing"
    );
}

#[test]
fn test_concurrent_file_access() {
    let ctx = TestContext::with_test_accounts();

    // Simulate concurrent access by running multiple commands quickly
    let handles: Vec<_> = (0..3)
        .map(|_| {
            let file_path = ctx.file_path().to_string_lossy().to_string();
            std::thread::spawn(move || {
                run_hotpot_command(&["--file", &file_path, "code", "github"])
            })
        })
        .collect();

    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    // At least some should succeed (file locking might cause some to fail)
    let success_count = results.iter().filter(|r| r.status.success()).count();
    assert!(
        success_count > 0,
        "At least one concurrent access should succeed"
    );
}

#[test]
fn test_file_persistence_across_commands() {
    let ctx = TestContext::with_test_accounts();
    let initial_count = get_account_count(ctx.file_path());

    // Run multiple read commands
    for _ in 0..3 {
        let output = run_hotpot_command(&[
            "--file",
            ctx.file_path().to_str().unwrap(),
            "code",
            "github",
        ]);

        if output.status.success() {
            // File should remain unchanged after read operations
            assert_eq!(
                get_account_count(ctx.file_path()),
                initial_count,
                "File should not be modified by read operations"
            );
        }
    }
}

#[test]
fn test_large_file_handling() {
    let ctx = TestContext::new();

    // Create a file with many accounts
    let mut accounts = Vec::new();
    for i in 0..100 {
        let account_data = serde_json::json!({
            "name": format!("account{}", i),
            "secret": "JBSWY3DPEHPK3PXP",
            "issuer": "",
            "algorithm": "SHA1",
            "digits": 6,
            "period": 30,
            "epoch": 0
        });
        accounts.push(account_data);
    }

    let storage = serde_json::json!({"accounts": accounts});
    let json_data = serde_json::to_string_pretty(&storage).unwrap();
    fs::write(ctx.file_path(), json_data).expect("Failed to write large file");

    // Test that we can still access accounts in large files
    let output = run_hotpot_command(&[
        "--file",
        ctx.file_path().to_str().unwrap(),
        "code",
        "account50",
    ]);

    assert!(
        output.status.success(),
        "Should handle large files correctly"
    );
}

#[test]
fn test_special_characters_in_account_names() {
    let ctx = TestContext::new();

    let special_data = r#"{
  "accounts": [
    {
      "name": "test@example.com",
      "secret": "JBSWY3DPEHPK3PXP",
      "issuer": "",
      "algorithm": "SHA1",
      "digits": 6,
      "period": 30,
      "epoch": 0
    },
    {
      "name": "test-account_123",
      "secret": "HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ",
      "issuer": "",
      "algorithm": "SHA1",
      "digits": 6,
      "period": 30,
      "epoch": 0
    }
  ]
}"#;

    fs::write(ctx.file_path(), special_data).expect("Failed to write special character data");

    let output1 = run_hotpot_command(&[
        "--file",
        ctx.file_path().to_str().unwrap(),
        "code",
        "test@example.com",
    ]);

    let output2 = run_hotpot_command(&[
        "--file",
        ctx.file_path().to_str().unwrap(),
        "code",
        "test-account_123",
    ]);

    assert!(
        output1.status.success(),
        "Should handle email-like account names"
    );
    assert!(
        output2.status.success(),
        "Should handle accounts with special characters"
    );
}
