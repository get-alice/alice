use alice_core::{Compose, MessagePatch, MessageQuery, providers, store::Store};
use std::{collections::HashSet, path::Path, time::Instant};
use tower::ServiceExt;
fn main() -> anyhow::Result<()> {
    let root=std::env::args().nth(1).expect("fixture directory");
    let store=Store::open(&Path::new(&root).join("mail.sqlite3"))?;
    let demo=store.account("demo-work")?;
    let mut a=store.message("demo-1")?;
    a.id="offset-a".into(); a.folder="archive".into(); a.date="2026-10-11T08:00:00+02:00".into(); store.save_message(&a)?;
    a.id="offset-b".into(); a.date="2026-10-11T07:00:00+00:00".into(); store.save_message(&a)?;
    let page=store.list(&MessageQuery {folder:Some("archive".into()),..Default::default()})?;
    println!("timezone_actual_first={} expected=offset-b",page.messages[0].id);
    let raw=b"From: Notifications <notifications@example.invalid>\r\nReply-To: Support <support@example.invalid>\r\nTo: me@example.invalid\r\nSubject: Ticket update\r\nMessage-ID: <reply-to@example.invalid>\r\nContent-Type: text/plain\r\n\r\nPlease reply to support.";
    let parsed=providers::parse_message(&demo,"inbox","INBOX",7,1,raw)?;
    println!("reply_target={} expected=support@example.invalid",parsed.from_email);
    store.save_message(&parsed)?;
    println!("reply_message_id={}",parsed.id);
    let mixed=b"From: Support <support@example.invalid>\r\nTo: me@example.invalid\r\nSubject: Two important sections\r\nMessage-ID: <mixed@example.invalid>\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\nContent-Type: text/plain\r\n\r\nFirst visible section.\r\n--x\r\nContent-Type: text/plain\r\n\r\nSecond important section.\r\n--x--\r\n";
    let mixed=providers::parse_message(&demo,"inbox","INBOX",7,2,mixed)?;
    println!("mime_first_present={} mime_second_present={}",mixed.body.contains("First visible"),mixed.body.contains("Second important"));
    let mut draft=Compose{id:"00000000-0000-4000-8000-000000000001".into(),account_id:demo.id.clone(),to:vec!["support@example.invalid".into()],subject:"Concurrent draft".into(),body:"Ready to send".into(),in_reply_to:None};
    let before=store.save_draft(&draft)?; providers::build_mail(&demo,&before)?;
    draft.to.clear(); store.save_draft(&draft)?;
    let claimed=store.claim_delivery(&draft.id)?;
    let validation_fails=providers::build_mail(&demo,&store.message(&draft.id)?).is_err();
    let edit_blocked=store.save_draft(&draft).is_err();
    println!("send_edit_race_claimed={} second_validation_fails={} future_edits_blocked={}",claimed,validation_fails,edit_blocked);
    draft.id="00000000-0000-4000-8000-000000000002".into(); draft.to.push("support@example.invalid".into());
    let before=store.save_draft(&draft)?; providers::build_mail(&demo,&before)?;
    store.discard_draft(&draft.id)?;
    println!("send_discard_race_claimed={} message_missing={}",store.claim_delivery(&draft.id)?,store.message(&draft.id).is_err());
    let mut real=demo.clone(); real.id="fixture-real".into(); real.name="Fixture".into();real.email="me@example.invalid".into();real.is_demo=false;store.save_account(&real)?;
    let moved=providers::parse_message(&real,"inbox","INBOX",7,11,b"From: support@example.invalid\r\nTo: me@example.invalid\r\nMessage-ID: <moved@example.invalid>\r\nSubject: Moved message\r\n\r\nHello")?;
    store.save_message(&moved)?;store.patch(&moved.id,&MessagePatch{folder:Some("archive".into()),..Default::default()})?;
    store.reconcile(&real.id,&[("INBOX".into(),7,HashSet::new()),("Archive".into(),8,HashSet::new())])?;
    println!("move_then_remote_delete_cached={} remote_uid={:?}",store.message(&moved.id).is_ok(),store.message(&moved.id)?.remote_uid);
    draft.id="00000000-0000-4000-8000-000000000003".into();draft.account_id=real.id.clone();
    let sent=store.save_draft(&draft)?;store.claim_delivery(&draft.id)?;store.finish_delivery(&draft.id,true)?;
    let raw=format!("From: me@example.invalid\r\nTo: support@example.invalid\r\nSubject: Concurrent draft\r\nMessage-ID: {}\r\n\r\nReady to send",sent.message_id.clone().unwrap());
    let provider_sent=providers::parse_message(&real,"sent","Sent",9,12,raw.as_bytes())?;
    store.save_message(&provider_sent)?;
    println!("sent_same_message_id_count={}",store.list(&MessageQuery{folder:Some("sent".into()),..Default::default()})?.total);
    let mut esc=store.message("demo-1")?;esc.id="terminal-escape".into();esc.body="Before\u{1b}[2J\u{1b}[HFORGED TERMINAL CONTENT".into();esc.is_read=true;store.save_message(&esc)?;
    let shared=std::sync::Arc::new(store);
    let runtime=tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        let response=alice_core::daemon::router(shared.clone(),"fixture-token".into()).oneshot(axum::http::Request::builder().method("PATCH").uri(format!("/v1/messages/{}",provider_sent.id)).header("Authorization","Bearer fixture-token").header("Content-Type","application/json").body(axum::body::Body::from(r#"{"is_starred":true}"#))?).await?;
        println!("sent_real_no_credentials_star_status={}",response.status());
        Ok::<_,anyhow::Error>(())
    })?;
    let mut bench=shared.message("demo-1")?;bench.account_id=real.id;bench.body="abcdefgh".repeat(1024);bench.folder="inbox".into();
    for i in 0..10000 {bench.id=format!("bench-{i}");shared.save_message(&bench)?;}
    let at=Instant::now();
    for _ in 0..3 {let _=shared.list(&MessageQuery{folder:Some("inbox".into()),limit:Some(1),..Default::default()})?;}
    println!("three_poll_scans_10000_8k_ms={}",at.elapsed().as_millis());
    Ok(())
}
