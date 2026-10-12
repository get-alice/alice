use alice_core::{Account, Mailboxes, Provider, remote::Remote, vault::Secret};
use anyhow::Result;

#[test]
#[ignore = "requires isolated scripts/mail-fixture.sh servers and their trusted test certificate"]
fn gmail_discovers_localized_all_mail_from_special_use() -> Result<()> {
    let account = Account {
        id: "gmail-folder-contract".into(),
        name: "Gmail protocol fixture".into(),
        email: "alice@example.test".into(),
        provider: Provider::Gmail,
        is_demo: false,
        imap_host: "localhost".into(),
        imap_port: 993,
        smtp_host: "localhost".into(),
        smtp_port: 587,
        folders: Mailboxes::default(),
        last_synced: None,
        sync_error: None,
        integrations: Default::default(),
    };
    let mut remote = Remote::connect(
        &account,
        &Secret {
            password: Some("alice-fixture-password".into()),
            ..Default::default()
        },
    )?;
    let (folders, synced) = remote.folders()?;
    remote.logout();
    assert_eq!(folders.archive, "Alle Nachrichten");
    assert!(
        synced
            .iter()
            .any(|(folder, mailbox)| folder == "archive" && mailbox == "Alle Nachrichten")
    );
    Ok(())
}
