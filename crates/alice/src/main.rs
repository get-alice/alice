#![forbid(unsafe_code)]

mod mcp;
mod tui;
use alice_core::{
    Account, Client, Compose, EventDraft, Invite, Message, MessagePage, MessagePatch, Rsvp,
    i18n::{self, Locale},
};
use anyhow::{Context, Result, bail};
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use serde_json::{Value, json};
use std::sync::{LazyLock, OnceLock};
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "alice", version = alice_core::client::VERSION)]
struct Args {
    #[command(subcommand)]
    command: Option<Action>,
}
#[derive(Subcommand)]
enum Action {
    Daemon {
        #[command(subcommand)]
        command: DaemonAction,
    },
    List {
        #[arg(long, default_value = "inbox")]
        folder: String,
        #[arg(long)]
        account: Option<String>,
        #[arg(long)]
        search: Option<String>,
        #[arg(long)]
        unread: bool,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        #[arg(long)]
        json: bool,
    },
    Read {
        id: String,
        #[arg(long)]
        json: bool,
    },
    Reply {
        id: String,
        #[arg(long, conflicts_with = "body_file")]
        body: Option<String>,
        #[arg(long)]
        body_file: Option<std::path::PathBuf>,
        #[arg(long)]
        send: bool,
    },
    Compose {
        #[arg(long)]
        account: Option<String>,
        #[arg(long, value_delimiter = ',')]
        to: Vec<String>,
        #[arg(long)]
        subject: String,
        #[arg(long, conflicts_with = "body_file")]
        body: Option<String>,
        #[arg(long)]
        body_file: Option<std::path::PathBuf>,
        #[arg(long)]
        send: bool,
    },
    Send {
        id: String,
    },
    Discard {
        id: String,
    },
    Archive {
        id: String,
    },
    Trash {
        id: String,
    },
    Spam {
        id: String,
    },
    Restore {
        id: String,
    },
    Star {
        id: String,
        #[arg(long)]
        remove: bool,
    },
    Unread {
        id: String,
    },
    Sync,
    Accounts {
        #[command(subcommand)]
        command: Option<AccountAction>,
    },
    Ai {
        #[command(subcommand)]
        command: AiAction,
    },
    Contacts {
        #[command(subcommand)]
        command: ContactsAction,
    },
    Dav {
        #[command(subcommand)]
        command: DavAction,
    },
    Invite {
        id: String,
        #[arg(value_enum)]
        response: Answer,
        #[arg(long)]
        comment: Option<String>,
    },
    Calendar {
        #[command(subcommand)]
        command: CalendarAction,
    },
    Mcp {
        #[arg(value_enum)]
        access: Option<McpMode>,
    },
}
#[derive(Clone, Copy, ValueEnum)]
enum McpMode {
    Off,
    ReadOnly,
    Full,
}
#[derive(Clone, Copy, ValueEnum)]
enum Toggle {
    On,
    Off,
}
#[derive(Clone, Copy, ValueEnum)]
enum Answer {
    Accept,
    Tentative,
    Decline,
}
impl From<Answer> for Rsvp {
    fn from(answer: Answer) -> Self {
        match answer {
            Answer::Accept => Rsvp::Accepted,
            Answer::Tentative => Rsvp::Tentative,
            Answer::Decline => Rsvp::Declined,
        }
    }
}
#[derive(Clone, Copy, ValueEnum)]
enum Kind {
    Carddav,
    Caldav,
}
#[derive(Subcommand)]
enum ContactsAction {
    Search {
        query: String,
        #[arg(long)]
        account: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    Sync,
}
#[derive(Subcommand)]
enum DavAction {
    List,
    Add {
        #[arg(long, value_enum)]
        kind: Kind,
        #[arg(long)]
        name: String,
        #[arg(long)]
        url: String,
        #[arg(long)]
        username: String,
    },
    Remove {
        source: String,
    },
}
#[derive(Subcommand)]
enum CalendarAction {
    List {
        #[arg(long)]
        json: bool,
    },
    Busy {
        #[arg(long, default_value_t = 7, value_parser = clap::value_parser!(u32).range(1..=62))]
        days: u32,
        #[arg(long)]
        json: bool,
    },
    Add {
        id: String,
        #[arg(long)]
        calendar: Option<String>,
    },
    Export {
        id: String,
        #[arg(long)]
        output: Option<std::path::PathBuf>,
        #[arg(long)]
        open: bool,
    },
}
#[derive(Subcommand)]
enum DaemonAction {
    Run,
    Status,
    Stop,
}
#[derive(Subcommand)]
enum AccountAction {
    Add {
        #[arg(long)]
        provider: String,
        #[arg(long)]
        email: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        client_id: Option<String>,
        #[arg(long)]
        imap_host: Option<String>,
        #[arg(long, default_value_t = 993)]
        imap_port: u16,
        #[arg(long)]
        smtp_host: Option<String>,
        #[arg(long, default_value_t = 587)]
        smtp_port: u16,
        #[arg(long)]
        app_password: bool,
    },
    Remove {
        account: String,
        #[arg(long)]
        yes: bool,
    },
    Access {
        account: String,
        #[arg(long, value_enum)]
        contacts: Option<Toggle>,
        #[arg(long, value_enum)]
        calendar: Option<Toggle>,
    },
    Demo,
}
#[derive(Subcommand)]
enum AiAction {
    Configure {
        #[arg(long, default_value = "openai")]
        provider: String,
        #[arg(long)]
        model: String,
        #[arg(long)]
        base_url: Option<String>,
    },
    Ask {
        prompt: String,
        #[arg(long)]
        message: Option<String>,
        #[arg(long)]
        json: bool,
    },
}
#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{}", terminal_text(&i18n::error_text(&error, locale())));
        std::process::exit(1);
    }
}
pub(crate) fn locale() -> Locale {
    static LOCALE: OnceLock<Locale> = OnceLock::new();
    *LOCALE.get_or_init(Locale::from_env)
}
pub(crate) fn terminal_text(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect()
}
fn localized_command(mut command: clap::Command, path: &str) -> clap::Command {
    let key = if path.is_empty() { "root" } else { path };
    command = command.about(i18n::tr(locale(), &format!("cli.help.commands.{key}"), &[]));
    let usage = command.render_usage().to_string();
    let usage = usage
        .strip_prefix("Usage: ")
        .unwrap_or(&usage)
        .replace(
            "[OPTIONS]",
            &alice_core::t!(locale(), "cli.help.options_token"),
        )
        .replace(
            "<COMMAND>",
            &alice_core::t!(locale(), "cli.help.command_required_token"),
        )
        .replace(
            "[COMMAND]",
            &alice_core::t!(locale(), "cli.help.command_optional_token"),
        );
    command = command
        .override_usage(usage)
        .help_template(alice_core::t!(locale(), "cli.help.template"))
        .subcommand_help_heading(alice_core::t!(locale(), "cli.help.commands_heading"))
        .mut_args(|arg| {
            let id = arg.get_id().as_str();
            let scoped = format!("cli.help.scoped.{key}.{id}");
            let text_key = if i18n::exists(&scoped) {
                scoped
            } else {
                format!("cli.help.args.{id}")
            };
            let mut help = i18n::tr(locale(), &text_key, &[]);
            let defaults = arg
                .get_default_values()
                .iter()
                .map(|value| value.to_string_lossy())
                .collect::<Vec<_>>()
                .join(", ");
            if arg.get_action().takes_values() && !defaults.is_empty() {
                help.push(' ');
                help.push_str(&alice_core::t!(
                    locale(),
                    "cli.help.default_value",
                    value = defaults
                ));
            }
            if let Some(values) = arg.get_value_parser().possible_values() {
                let values = values
                    .filter(|value| !value.is_hide_set())
                    .map(|value| value.get_name().to_owned())
                    .collect::<Vec<_>>()
                    .join(", ");
                if !values.is_empty() {
                    help.push(' ');
                    help.push_str(&alice_core::t!(
                        locale(),
                        "cli.help.possible_values",
                        values = values
                    ));
                }
            }
            let heading = if arg.is_positional() {
                alice_core::t!(locale(), "cli.help.arguments_heading")
            } else {
                alice_core::t!(locale(), "cli.help.options_heading")
            };
            arg.long_help(None::<&str>)
                .help(help)
                .help_heading(heading)
                .hide_default_value(true)
                .hide_possible_values(true)
        });
    command.mut_subcommands(|subcommand| {
        let child = if subcommand.get_name() == "help" {
            "help".to_owned()
        } else if path.is_empty() {
            subcommand.get_name().to_owned()
        } else {
            format!("{path}_{}", subcommand.get_name())
        };
        localized_command(subcommand, &child)
    })
}
async fn run() -> Result<()> {
    let mut command = Args::command();
    command.build();
    let matches = localized_command(command, "").get_matches();
    let args = Args::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());
    if let Some(Action::Daemon { command }) = &args.command {
        match command {
            DaemonAction::Run => return alice_core::daemon::run().await,
            DaemonAction::Status => {
                let client = Client::existing()?.with_locale(locale());
                println!("{}", client.get::<Value>("/v1/health").await?);
            }
            DaemonAction::Stop => {
                let client = Client::existing()?.with_locale(locale());
                let _: Value = client.post("/v1/shutdown", &json!({})).await?;
                println!("{}", alice_core::t!(locale(), "cli.daemon_stopping"));
            }
        }
        return Ok(());
    }
    let client = Client::connect(&["daemon", "run"])
        .await?
        .with_locale(locale());
    match args.command {
        None => tui::run(client).await?,
        Some(Action::Mcp { access: None }) => mcp::serve(client).await?,
        Some(Action::Mcp {
            access: Some(access),
        }) => {
            let (mode, key) = match access {
                McpMode::Off => ("off", "cli.mcp_off"),
                McpMode::ReadOnly => ("read_only", "cli.mcp_read_only_on"),
                McpMode::Full => ("full", "cli.mcp_full"),
            };
            let _: alice_core::Preferences = client
                .patch("/v1/preferences", &json!({ "mcp": mode }))
                .await?;
            println!("{}", i18n::tr(locale(), key, &[]));
        }
        Some(Action::List {
            folder,
            account,
            search,
            unread,
            limit,
            offset,
            json,
        }) => {
            let mut path = format!(
                "/v1/messages?folder={}&limit={limit}&offset={offset}",
                encode(&folder)
            );
            if let Some(id) = account {
                path.push_str(&format!("&account_id={}", encode(&id)));
            }
            if let Some(q) = search {
                path.push_str(&format!("&q={}", encode(&q)));
            }
            if unread {
                path.push_str("&unread=true");
            }
            let page: MessagePage = client.get(&path).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&page)?);
            } else {
                for m in page.messages {
                    println!(
                        "{} {}  {:20}  {}",
                        if m.is_read { " " } else { "●" },
                        m.id,
                        terminal_text(&m.from_name),
                        terminal_text(&m.subject)
                    );
                }
                println!(
                    "{}",
                    alice_core::t!(locale(), "cli.messages", count = page.total)
                );
            }
        }
        Some(Action::Read { id, json }) => {
            let mut m: Message = client.get(&format!("/v1/messages/{}", encode(&id))).await?;
            match client
                .patch::<Message>(
                    &format!("/v1/messages/{}", encode(&id)),
                    &MessagePatch {
                        is_read: Some(true),
                        ..Default::default()
                    },
                )
                .await
            {
                Ok(updated) => m = updated,
                Err(error) => {
                    eprintln!(
                        "{}",
                        alice_core::t!(
                            locale(),
                            "cli.read_cached",
                            error = terminal_text(&i18n::error_text(&error, locale()))
                        )
                    )
                }
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&m)?);
            } else {
                println!(
                    "{}",
                    alice_core::t!(
                        locale(),
                        "cli.message_header",
                        subject = terminal_text(&m.subject),
                        name = terminal_text(&m.from_name),
                        email = terminal_text(&m.from_email),
                        recipients = terminal_text(&m.to.join(", ")),
                        date = terminal_text(&m.date),
                    )
                );
                if let Some(invite) = &m.invite {
                    println!("\n{}", invite_summary(&m.id, invite));
                }
                println!("\n{}", terminal_text(&m.body));
            }
        }
        Some(Action::Reply {
            id,
            body,
            body_file,
            send,
        }) => {
            let m: Message = client.get(&format!("/v1/messages/{}", encode(&id))).await?;
            save(&client, reply_compose(m, read_body(body, body_file)?), send).await?;
        }
        Some(Action::Compose {
            account,
            to,
            subject,
            body,
            body_file,
            send,
        }) => {
            save(
                &client,
                Compose {
                    id: Uuid::new_v4().to_string(),
                    account_id: account.unwrap_or_default(),
                    to,
                    subject,
                    body: read_body(body, body_file)?,
                    in_reply_to: None,
                },
                send,
            )
            .await?;
        }
        Some(Action::Discard { id }) => {
            client
                .request_value("DELETE", &format!("/v1/messages/{}", encode(&id)), None)
                .await?;
            println!("{}", alice_core::t!(locale(), "cli.draft_discarded"));
        }
        Some(Action::Send { id }) => {
            let m: Message = client
                .post(&format!("/v1/messages/{}/send", encode(&id)), &json!({}))
                .await?;
            println!("{}", alice_core::t!(locale(), "cli.sent", id = m.id));
        }
        Some(Action::Archive { id }) => {
            update(
                &client,
                &id,
                MessagePatch {
                    folder: Some("archive".into()),
                    ..Default::default()
                },
            )
            .await?
        }
        Some(Action::Trash { id }) => {
            update(
                &client,
                &id,
                MessagePatch {
                    folder: Some("trash".into()),
                    ..Default::default()
                },
            )
            .await?
        }
        Some(Action::Spam { id }) => {
            update(
                &client,
                &id,
                MessagePatch {
                    folder: Some("spam".into()),
                    ..Default::default()
                },
            )
            .await?
        }
        Some(Action::Restore { id }) => {
            update(
                &client,
                &id,
                MessagePatch {
                    folder: Some("inbox".into()),
                    ..Default::default()
                },
            )
            .await?
        }
        Some(Action::Star { id, remove }) => {
            update(
                &client,
                &id,
                MessagePatch {
                    is_starred: Some(!remove),
                    ..Default::default()
                },
            )
            .await?
        }
        Some(Action::Unread { id }) => {
            update(
                &client,
                &id,
                MessagePatch {
                    is_read: Some(false),
                    ..Default::default()
                },
            )
            .await?
        }
        Some(Action::Sync) => println!(
            "{}",
            serde_json::to_string_pretty(&client.post::<Value>("/v1/sync", &json!({})).await?)?
        ),
        Some(Action::Accounts { command: None }) => {
            for a in client.get::<Vec<Account>>("/v1/accounts").await? {
                println!(
                    "{}  {}  {:?}{}",
                    a.id,
                    terminal_text(&a.email),
                    a.provider,
                    if a.is_demo {
                        alice_core::t!(locale(), "cli.demo_suffix")
                    } else {
                        String::new()
                    }
                );
            }
        }
        Some(Action::Accounts {
            command:
                Some(AccountAction::Add {
                    provider,
                    email,
                    name,
                    client_id,
                    imap_host,
                    imap_port,
                    smtp_host,
                    smtp_port,
                    app_password,
                }),
        }) => {
            if !["gmail", "outlook", "imap"].contains(&provider.as_str()) {
                bail!(alice_core::text!("cli.provider_required"));
            }
            let password = if app_password || provider == "imap" {
                Some(rpassword::prompt_password(alice_core::t!(
                    locale(),
                    "cli.app_password_prompt"
                ))?)
            } else {
                None
            };
            let result:Value=client.post("/v1/accounts",&json!({"provider":provider,"email":email,"name":name,"client_id":client_id,"password":password,"imap_host":imap_host,"imap_port":imap_port,"smtp_host":smtp_host,"smtp_port":smtp_port})).await?;
            if result.get("url").is_some() {
                finish_in_browser(&client, &result).await?;
                println!("{}", alice_core::t!(locale(), "cli.account_connected"));
            } else {
                println!(
                    "{}",
                    alice_core::t!(
                        locale(),
                        "cli.connected",
                        email =
                            terminal_text(result["account"]["email"].as_str().unwrap_or_default())
                    )
                );
            }
        }
        Some(Action::Accounts {
            command: Some(AccountAction::Demo),
        }) => {
            let account: Account = client.post("/v1/demo", &json!({})).await?;
            println!(
                "{}",
                alice_core::t!(
                    locale(),
                    "cli.demo_ready",
                    id = account.id,
                    email = terminal_text(&account.email)
                )
            );
        }
        Some(Action::Accounts {
            command: Some(AccountAction::Remove { account, yes }),
        }) => {
            let target = find_account(&client, &account).await?;
            if !yes {
                use std::io::{IsTerminal, Write};
                if !std::io::stdin().is_terminal() {
                    bail!(alice_core::text!("cli.remove_requires_yes"));
                }
                print!(
                    "{}",
                    alice_core::t!(
                        locale(),
                        "cli.remove_prompt",
                        email = terminal_text(&target.email)
                    )
                );
                std::io::stdout().flush()?;
                let mut answer = String::new();
                std::io::stdin().read_line(&mut answer)?;
                if !answer.trim().eq_ignore_ascii_case("y") {
                    println!(
                        "{}",
                        alice_core::t!(locale(), "cli.kept", name = terminal_text(&target.email))
                    );
                    return Ok(());
                }
            }
            let result = client
                .request_value(
                    "DELETE",
                    &format!("/v1/accounts/{}", encode(&target.id)),
                    None,
                )
                .await?;
            println!(
                "{}",
                alice_core::t!(locale(), "cli.removed", name = terminal_text(&target.email))
            );
            if result["credentials_removed"] == false {
                println!(
                    "{}",
                    alice_core::t!(
                        locale(),
                        "cli.account_credentials_remain",
                        error = terminal_text(
                            result["message"]
                                .as_str()
                                .unwrap_or(&alice_core::t!(locale(), "cli.access_denied"))
                        )
                    )
                );
            }
        }
        Some(Action::Accounts {
            command:
                Some(AccountAction::Access {
                    account,
                    contacts,
                    calendar,
                }),
        }) => {
            let target = find_account(&client, &account).await?;
            if contacts.is_none() && calendar.is_none() {
                println!("{}", access_summary(&target));
                return Ok(());
            }
            let on = |toggle: Option<Toggle>| toggle.map(|t| matches!(t, Toggle::On));
            let result: Value = client
                .post(
                    &format!("/v1/accounts/{}/integrations", encode(&target.id)),
                    &json!({"contacts": on(contacts), "calendar": on(calendar)}),
                )
                .await?;
            let account = if result.get("url").is_some() {
                finish_in_browser(&client, &result).await?
            } else {
                result["account"].clone()
            };
            let account: Account = serde_json::from_value(account)
                .context(alice_core::text!("cli.missing_account"))?;
            println!("{}", access_summary(&account));
        }
        Some(Action::Ai {
            command:
                AiAction::Configure {
                    provider,
                    model,
                    base_url,
                },
        }) => {
            let api_key =
                rpassword::prompt_password(alice_core::t!(locale(), "cli.ai_key_prompt"))?;
            let base_url = base_url.unwrap_or_else(|| {
                if provider == "anthropic" {
                    "https://api.anthropic.com/v1".into()
                } else {
                    "https://api.openai.com/v1".into()
                }
            });
            let _:Value=client.post("/v1/ai/settings",&json!({"provider":provider,"model":model,"base_url":base_url,"api_key":api_key})).await?;
            println!("{}", alice_core::t!(locale(), "cli.ai_saved"));
        }
        Some(Action::Ai {
            command:
                AiAction::Ask {
                    prompt,
                    message,
                    json,
                },
        }) => {
            let result: alice_core::AiResult = client
                .post(
                    "/v1/ai",
                    &alice_core::AiRequest {
                        prompt,
                        message_id: message,
                    },
                )
                .await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("{}", terminal_text(&result.text));
                if let Some(draft) = result.draft {
                    println!(
                        "{}",
                        alice_core::t!(
                            locale(),
                            "cli.proposed_reply",
                            draft = terminal_text(&draft)
                        )
                    );
                }
                for action in result.actions {
                    let label = match action.action {
                        alice_core::InboxAction::Archive => {
                            alice_core::t!(locale(), "cli.action_archive")
                        }
                        alice_core::InboxAction::MarkRead => {
                            alice_core::t!(locale(), "cli.action_mark_read")
                        }
                        alice_core::InboxAction::Star => {
                            alice_core::t!(locale(), "cli.action_star")
                        }
                    };
                    println!(
                        "{}",
                        alice_core::t!(
                            locale(),
                            "cli.proposed_action",
                            action = label,
                            id = action.message_id
                        )
                    );
                }
                if let Some(event) = result.event {
                    println!(
                        "{}",
                        alice_core::t!(
                            locale(),
                            "cli.proposed_event",
                            title = event_title(&event),
                            when = event_when(&event)
                        )
                    );
                    if let Some(location) =
                        event.location.as_deref().filter(|l| !l.trim().is_empty())
                    {
                        println!(
                            "{}",
                            alice_core::t!(
                                locale(),
                                "cli.event_where",
                                location = terminal_text(location)
                            )
                        );
                    }
                }
            }
        }
        Some(Action::Contacts {
            command:
                ContactsAction::Search {
                    query,
                    account,
                    limit,
                    json,
                },
        }) => {
            let mut path = format!("/v1/contacts?q={}", encode(&query));
            if let Some(account) = account {
                let id = find_account(&client, &account).await?.id;
                path.push_str(&format!("&account_id={}", encode(&id)));
            }
            if let Some(limit) = limit {
                path.push_str(&format!("&limit={limit}"));
            }
            let contacts: Vec<alice_core::Contact> = client.get(&path).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&contacts)?);
            } else if contacts.is_empty() {
                println!(
                    "{}",
                    alice_core::t!(locale(), "cli.no_contacts", query = terminal_text(&query))
                );
            } else {
                for contact in contacts {
                    let sources = contact
                        .sources
                        .iter()
                        .map(|source| source_name(*source))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let email = terminal_text(&contact.email);
                    if contact.name.trim().is_empty() {
                        println!("{email}  ({sources})");
                    } else {
                        println!("{} <{email}>  ({sources})", terminal_text(&contact.name));
                    }
                }
            }
        }
        Some(Action::Contacts {
            command: ContactsAction::Sync,
        }) => {
            let result: alice_core::ContactsSync =
                client.post("/v1/contacts/sync", &json!({})).await?;
            println!(
                "{}",
                alice_core::t!(locale(), "cli.contacts_refreshed", count = result.contacts)
            );
            for error in result.errors {
                eprintln!(
                    "{}",
                    alice_core::t!(
                        locale(),
                        "cli.refresh_failed",
                        source = terminal_text(&error.source),
                        error = terminal_text(&error.message)
                    )
                );
            }
        }
        Some(Action::Dav {
            command: DavAction::List,
        }) => {
            let sources: Vec<alice_core::DavSource> = client.get("/v1/dav").await?;
            if sources.is_empty() {
                println!("{}", alice_core::t!(locale(), "cli.no_dav"));
            }
            for source in sources {
                println!(
                    "{}",
                    alice_core::t!(
                        locale(),
                        "cli.dav_line",
                        id = source.id,
                        kind = match source.kind {
                            alice_core::DavKind::Carddav => "CardDAV",
                            alice_core::DavKind::Caldav => "CalDAV",
                        },
                        name = terminal_text(&source.name),
                        username = terminal_text(&source.username),
                        url = terminal_text(&source.url),
                        error = source
                            .error
                            .map(|error| format!("  · {}", terminal_text(&error)))
                            .unwrap_or_default()
                    )
                );
            }
        }
        Some(Action::Dav {
            command:
                DavAction::Add {
                    kind,
                    name,
                    url,
                    username,
                },
        }) => {
            let password = rpassword::prompt_password(alice_core::t!(
                locale(),
                "cli.password_prompt",
                username = terminal_text(&username)
            ))?;
            let source: alice_core::DavSource = client
                .post(
                    "/v1/dav",
                    &alice_core::AddDavSource {
                        kind: match kind {
                            Kind::Carddav => alice_core::DavKind::Carddav,
                            Kind::Caldav => alice_core::DavKind::Caldav,
                        },
                        name,
                        url,
                        username,
                        password,
                    },
                )
                .await?;
            println!(
                "{}",
                alice_core::t!(
                    locale(),
                    "cli.dav_added",
                    name = terminal_text(&source.name),
                    id = source.id
                )
            );
        }
        Some(Action::Dav {
            command: DavAction::Remove { source },
        }) => {
            let sources: Vec<alice_core::DavSource> = client.get("/v1/dav").await?;
            let target = sources
                .iter()
                .find(|s| s.id == source || s.name.eq_ignore_ascii_case(&source))
                .context(alice_core::text!("cli.server_not_found"))?;
            let result = client
                .request_value("DELETE", &format!("/v1/dav/{}", encode(&target.id)), None)
                .await?;
            println!(
                "{}",
                alice_core::t!(locale(), "cli.removed", name = terminal_text(&target.name))
            );
            if result["credentials_removed"] == false {
                println!(
                    "{}",
                    alice_core::t!(
                        locale(),
                        "cli.dav_credentials_remain",
                        error = terminal_text(
                            result["message"]
                                .as_str()
                                .unwrap_or(&alice_core::t!(locale(), "cli.access_denied"))
                        ),
                        id = target.id.as_str()
                    )
                );
            }
        }
        Some(Action::Invite {
            id,
            response,
            comment,
        }) => {
            let m: Message = client
                .post(
                    &format!("/v1/messages/{}/rsvp", encode(&id)),
                    &alice_core::RsvpRequest {
                        response: response.into(),
                        comment,
                    },
                )
                .await?;
            let accounts: Vec<Account> = client.get("/v1/accounts").await?;
            let demo = accounts.iter().any(|a| a.id == m.account_id && a.is_demo);
            println!("{}", rsvp_done(&m, demo));
        }
        Some(Action::Calendar {
            command: CalendarAction::List { json },
        }) => {
            let list: alice_core::CalendarList = client.get("/v1/calendars").await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&list)?);
            } else {
                let preferences: alice_core::Preferences = client.get("/v1/preferences").await?;
                println!("{}", calendar_lines(&list, &preferences.default_calendar));
            }
        }
        Some(Action::Calendar {
            command: CalendarAction::Busy { days, json },
        }) => {
            let from = chrono::Utc::now();
            let to = from + chrono::Duration::days(days.into());
            let stamp = |t: chrono::DateTime<chrono::Utc>| {
                encode(&t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
            };
            let report: alice_core::BusyReport = client
                .get(&format!(
                    "/v1/calendar/busy?from={}&to={}",
                    stamp(from),
                    stamp(to)
                ))
                .await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                if report.sources == 0 && report.errors.is_empty() {
                    println!("{}", alice_core::t!(locale(), "cli.no_calendars"));
                } else if report.busy.is_empty() {
                    println!(
                        "{}",
                        alice_core::t!(locale(), "cli.free_days", count = days)
                    );
                }
                for busy in &report.busy {
                    println!(
                        "{}",
                        alice_core::t!(
                            locale(),
                            "cli.busy",
                            when = local_range(&busy.start, Some(&busy.end))
                        )
                    );
                }
                for error in report.errors {
                    eprintln!(
                        "{}",
                        alice_core::t!(
                            locale(),
                            "cli.read_failed",
                            source = terminal_text(&error.source),
                            error = terminal_text(&error.message)
                        )
                    );
                }
            }
        }
        Some(Action::Calendar {
            command: CalendarAction::Add { id, calendar },
        }) => {
            let calendar = match calendar {
                Some(calendar) => calendar,
                None => {
                    let preferences: alice_core::Preferences =
                        client.get("/v1/preferences").await?;
                    if preferences.default_calendar.is_empty() {
                        let list: alice_core::CalendarList = client.get("/v1/calendars").await?;
                        bail!(alice_core::text!(
                            "cli.choose_calendar",
                            calendars = calendar_lines(&list, ""),
                            id = terminal_text(&id)
                        ));
                    }
                    preferences.default_calendar
                }
            };
            let created: alice_core::CreatedEvent = client
                .post(
                    "/v1/calendar/events",
                    &alice_core::AddEvent {
                        calendar,
                        message_id: Some(id),
                        event: None,
                    },
                )
                .await?;
            println!("{}", alice_core::t!(locale(), "cli.calendar_added"));
            if let Some(url) = created.url {
                println!("{}", terminal_text(&url));
            }
        }
        Some(Action::Calendar {
            command: CalendarAction::Export { id, output, open },
        }) => {
            let file: alice_core::EventFile = client
                .post(
                    "/v1/calendar/ics",
                    &alice_core::EventFileRequest {
                        message_id: Some(id),
                        event: None,
                    },
                )
                .await?;
            // The daemon already suggests a safe name; keep only its last component regardless.
            let name = std::path::Path::new(&file.filename)
                .file_name()
                .map(|name| name.to_owned())
                .unwrap_or_else(|| "invite.ics".into());
            let path = match output {
                Some(path) => path,
                None if open => std::env::temp_dir().join(name),
                None => name.into(),
            };
            std::fs::write(&path, &file.ics).with_context(|| {
                alice_core::text!("cli.write_failed", path = path.display().to_string())
            })?;
            println!(
                "{}",
                alice_core::t!(
                    locale(),
                    "cli.saved",
                    path = terminal_text(&path.display().to_string())
                )
            );
            if open {
                open::that(&path).context(alice_core::text!("cli.open_calendar_failed"))?;
                println!("{}", alice_core::t!(locale(), "cli.calendar_opened"));
            }
        }
        Some(Action::Daemon { .. }) => unreachable!(),
    }
    Ok(())
}
pub(crate) fn reply_compose(m: Message, body: String) -> Compose {
    Compose {
        id: Uuid::new_v4().to_string(),
        to: m.reply_recipients(),
        account_id: m.account_id,
        subject: if m.subject.to_lowercase().starts_with("re:") {
            m.subject
        } else {
            format!("Re: {}", m.subject)
        },
        body,
        in_reply_to: m.message_id,
    }
}
fn read_body(body: Option<String>, file: Option<std::path::PathBuf>) -> Result<String> {
    body.map(Ok).unwrap_or_else(|| {
        file.map(std::fs::read_to_string)
            .transpose()
            .map(|s| s.unwrap_or_default())
            .map_err(Into::into)
    })
}
pub fn encode(value: &str) -> String {
    let mut url = reqwest::Url::parse("http://localhost/").unwrap();
    url.query_pairs_mut().append_pair("v", value);
    url.query().unwrap()[2..].to_string()
}
async fn find_account(client: &Client, account: &str) -> Result<Account> {
    let accounts: Vec<Account> = client.get("/v1/accounts").await?;
    accounts
        .into_iter()
        .find(|a| a.id == account || a.email.eq_ignore_ascii_case(account))
        .context(alice_core::text!("cli.account_not_found"))
}
/// Opens the consent page the daemon returned (`{flow_id, url}`) and waits until the OAuth flow
/// finishes. Returns the connected account as JSON.
async fn finish_in_browser(client: &Client, started: &Value) -> Result<Value> {
    let url = started["url"]
        .as_str()
        .context(alice_core::text!("cli.missing_signin_url"))?;
    println!(
        "{}",
        alice_core::t!(locale(), "cli.browser_signin", url = terminal_text(url))
    );
    let _ = open::that(url);
    let id = started["flow_id"]
        .as_str()
        .context(alice_core::text!("cli.missing_connection_id"))?;
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let flow: Value = client
            .get(&format!("/v1/connections/{}", encode(id)))
            .await?;
        match flow["status"].as_str() {
            Some("connected") => return Ok(flow["account"].clone()),
            Some("error") => match flow["message"].as_str() {
                Some(message) => bail!("{}", message),
                None => bail!(alice_core::text!("cli.signin_failed")),
            },
            _ => {}
        }
    }
}
fn access_summary(account: &Account) -> String {
    let state = |on: bool| {
        if on {
            alice_core::t!(locale(), "cli.access_on")
        } else {
            alice_core::t!(locale(), "cli.access_off")
        }
    };
    alice_core::t!(
        locale(),
        "cli.access_summary",
        email = terminal_text(&account.email),
        contacts = state(account.integrations.contacts),
        calendar = state(account.integrations.calendar)
    )
}
fn source_name(source: alice_core::ContactSource) -> &'static str {
    static SOURCES: LazyLock<[String; 2]> = LazyLock::new(|| {
        [
            alice_core::t!(locale(), "cli.source_mail"),
            alice_core::t!(locale(), "cli.source_system"),
        ]
    });
    match source {
        alice_core::ContactSource::Mail => &SOURCES[0],
        alice_core::ContactSource::Google => "Google",
        alice_core::ContactSource::Microsoft => "Microsoft",
        alice_core::ContactSource::Carddav => "CardDAV",
        alice_core::ContactSource::System => &SOURCES[1],
    }
}
/// One calendar per line, for `calendar list` and for choosing one in `calendar add`.
fn calendar_lines(list: &alice_core::CalendarList, default: &str) -> String {
    let mut lines: Vec<String> = list
        .calendars
        .iter()
        .map(|c| {
            format!(
                "{}  {} ({}){}{}",
                terminal_text(&c.id),
                terminal_text(&c.name),
                match c.source {
                    alice_core::CalendarSource::Google => "Google",
                    alice_core::CalendarSource::Microsoft => "Microsoft",
                    alice_core::CalendarSource::Caldav => "CalDAV",
                },
                if c.primary {
                    alice_core::t!(locale(), "cli.primary_suffix")
                } else {
                    String::new()
                },
                if c.id == default {
                    alice_core::t!(locale(), "cli.default_suffix")
                } else {
                    String::new()
                }
            )
        })
        .collect();
    if list.calendars.is_empty() {
        lines.push(alice_core::t!(locale(), "cli.no_calendars"));
    }
    lines.extend(list.errors.iter().map(|error| {
        alice_core::t!(
            locale(),
            "cli.list_failed",
            source = terminal_text(&error.source),
            error = terminal_text(&error.message)
        )
    }));
    lines.join("\n")
}
pub(crate) fn event_title(event: &EventDraft) -> String {
    let title = terminal_text(event.title.trim());
    if title.is_empty() {
        alice_core::t!(locale(), "cli.untitled_event")
    } else {
        title
    }
}
/// When an event happens, in local time. All-day events carry an exclusive iCalendar end date, so
/// a one-day event ends the next day and shows as a single date.
pub(crate) fn event_when(event: &EventDraft) -> String {
    event_when_in(event, locale())
}
fn event_when_in(event: &EventDraft, language: Locale) -> String {
    if !event.all_day {
        return local_range_in(&event.start, event.end.as_deref(), language);
    }
    let day = |value: &str| chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").ok();
    let Some(start) = day(&event.start) else {
        return terminal_text(&event.start);
    };
    let last = event
        .end
        .as_deref()
        .and_then(day)
        .and_then(|end| end.pred_opt())
        .filter(|last| *last > start);
    let date = |value: chrono::NaiveDate, key: &str| {
        i18n::date(
            language,
            &value.and_hms_opt(0, 0, 0).unwrap().and_utc(),
            &i18n::tr(language, key, &[]),
        )
    };
    match last {
        Some(last) => alice_core::t!(
            language,
            "cli.all_day_range",
            start = date(start, "cli.date.day_short"),
            end = date(last, "cli.date.day")
        ),
        None => alice_core::t!(language, "cli.all_day", date = date(start, "cli.date.day")),
    }
}
/// `Tue, Oct 13 2026 · 14:00–15:00` in local time; the end date is repeated only when it differs.
/// Anything that is not RFC 3339 is shown as-is.
pub(crate) fn local_range(start: &str, end: Option<&str>) -> String {
    local_range_in(start, end, locale())
}
fn local_range_in(start: &str, end: Option<&str>, language: Locale) -> String {
    let time = |value: &str| {
        chrono::DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|t| t.with_timezone(&chrono::Local))
    };
    let Some(from) = time(start) else {
        return terminal_text(start);
    };
    let first = i18n::date(
        language,
        &from,
        &alice_core::t!(language, "cli.date.date_time"),
    );
    match end.and_then(time) {
        Some(to) if to.date_naive() == from.date_naive() => {
            format!(
                "{first}–{}",
                i18n::date(language, &to, &alice_core::t!(language, "cli.date.time"))
            )
        }
        Some(to) => format!(
            "{first} – {}",
            i18n::date(
                language,
                &to,
                &alice_core::t!(language, "cli.date.date_time_short")
            )
        ),
        None => first,
    }
}
pub(crate) fn person(attendee: &alice_core::Attendee) -> String {
    match attendee
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
    {
        Some(name) => format!(
            "{} <{}>",
            terminal_text(name),
            terminal_text(&attendee.email)
        ),
        None => terminal_text(&attendee.email),
    }
}
/// Only a live request can be answered; replies, publications and cancellations cannot.
pub(crate) fn answerable(invite: &Invite) -> bool {
    !invite.cancelled && invite.method.eq_ignore_ascii_case("REQUEST")
}
pub(crate) fn invite_state(invite: &Invite) -> &'static str {
    static STATES: LazyLock<[String; 5]> = LazyLock::new(|| {
        [
            alice_core::t!(locale(), "cli.invite.cancelled"),
            alice_core::t!(locale(), "cli.invite.accepted"),
            alice_core::t!(locale(), "cli.invite.tentative"),
            alice_core::t!(locale(), "cli.invite.declined"),
            alice_core::t!(locale(), "cli.invite.unanswered"),
        ]
    });
    let index = if invite.cancelled {
        0
    } else {
        match invite.response {
            Some(Rsvp::Accepted) => 1,
            Some(Rsvp::Tentative) => 2,
            Some(Rsvp::Declined) => 3,
            None => 4,
        }
    };
    &STATES[index]
}
fn invite_summary(id: &str, invite: &Invite) -> String {
    let mut lines = vec![
        alice_core::t!(
            locale(),
            "cli.invite.title",
            title = event_title(&invite.event)
        ),
        alice_core::t!(
            locale(),
            "cli.invite.when",
            when = event_when(&invite.event)
        ),
    ];
    if let Some(location) = invite
        .event
        .location
        .as_deref()
        .map(str::trim)
        .filter(|l| !l.is_empty())
    {
        lines.push(alice_core::t!(
            locale(),
            "cli.invite.where",
            location = terminal_text(location)
        ));
    }
    if let Some(rule) = &invite.recurrence {
        lines.push(alice_core::t!(
            locale(),
            "cli.invite.repeats",
            rule = terminal_text(rule)
        ));
    }
    if let Some(organizer) = &invite.organizer {
        lines.push(alice_core::t!(
            locale(),
            "cli.invite.organizer",
            organizer = person(organizer)
        ));
    }
    let state = invite_state(invite);
    let you = if answerable(invite) {
        if invite.response.is_some() {
            alice_core::t!(
                locale(),
                "cli.invite.you_change",
                state = state,
                id = terminal_text(id)
            )
        } else {
            alice_core::t!(
                locale(),
                "cli.invite.you_answer",
                state = state,
                id = terminal_text(id)
            )
        }
    } else {
        alice_core::t!(locale(), "cli.invite.you", state = state)
    };
    lines.push(you);
    lines.join("\n")
}
/// What answering an invitation did, for the CLI and the TUI footer. The demo account records the
/// answer without mailing anyone.
pub(crate) fn rsvp_done(m: &Message, demo: bool) -> String {
    let Some(invite) = &m.invite else {
        return alice_core::t!(locale(), "cli.answer_saved");
    };
    if demo {
        return alice_core::t!(
            locale(),
            "cli.rsvp_demo",
            state = invite_state(invite),
            title = event_title(&invite.event)
        );
    }
    let organizer = invite
        .organizer
        .as_ref()
        .map_or(m.from_email.as_str(), |o| o.email.as_str());
    alice_core::t!(
        locale(),
        "cli.rsvp_sent",
        state = invite_state(invite),
        title = event_title(&invite.event),
        organizer = terminal_text(organizer)
    )
}
async fn update(client: &Client, id: &str, patch: MessagePatch) -> Result<()> {
    let m: Message = client
        .patch(&format!("/v1/messages/{}", encode(id)), &patch)
        .await?;
    let folder = match m.folder.as_str() {
        "inbox" => alice_core::t!(locale(), "common.folder.inbox"),
        "starred" => alice_core::t!(locale(), "common.folder.starred"),
        "sent" => alice_core::t!(locale(), "common.folder.sent"),
        "drafts" => alice_core::t!(locale(), "common.folder.drafts"),
        "archive" => alice_core::t!(locale(), "common.folder.archive"),
        "trash" => alice_core::t!(locale(), "common.folder.trash"),
        "spam" => alice_core::t!(locale(), "common.folder.spam"),
        _ => terminal_text(&m.folder),
    };
    println!(
        "{}",
        alice_core::t!(locale(), "cli.updated", id = m.id, folder = folder)
    );
    Ok(())
}
async fn save(client: &Client, mut draft: Compose, send: bool) -> Result<()> {
    draft.body = client
        .get::<alice_core::Preferences>("/v1/preferences")
        .await?
        .draft_body(&draft.body);
    let m: Message = client.post("/v1/messages", &draft).await?;
    if send {
        let _: Message = client
            .post(&format!("/v1/messages/{}/send", encode(&m.id)), &json!({}))
            .await?;
        println!("{}", alice_core::t!(locale(), "cli.sent", id = m.id));
    } else {
        println!(
            "{}",
            alice_core::t!(locale(), "cli.draft_saved", id = m.id.as_str())
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(start: &str, end: Option<&str>, all_day: bool) -> EventDraft {
        EventDraft {
            title: "Review".into(),
            start: start.into(),
            end: end.map(Into::into),
            all_day,
            location: None,
            description: None,
        }
    }

    #[test]
    fn all_day_events_end_the_day_before_their_exclusive_end() {
        let one_day = event("2026-10-13", Some("2026-10-14"), true);
        assert_eq!(
            event_when_in(&one_day, Locale::EnUs),
            "Tue, Oct 13 2026 · all day"
        );
        let three_days = event("2026-10-13", Some("2026-10-16"), true);
        assert_eq!(
            event_when_in(&three_days, Locale::EnUs),
            "Tue, Oct 13 – Thu, Oct 15 2026 · all day"
        );
        assert_eq!(
            event_when_in(&event("soon", None, true), Locale::EnUs),
            "soon"
        );
    }

    #[test]
    fn timed_events_repeat_the_date_only_across_days() {
        let same_day = event("2026-10-13T12:00:00Z", Some("2026-10-13T12:30:00Z"), false);
        let when = event_when_in(&same_day, Locale::EnUs);
        assert!(
            when.contains(" · ") && when.matches(", ").count() == 1,
            "{when}"
        );
        let overnight = event("2026-10-13T12:00:00Z", Some("2026-10-15T12:00:00Z"), false);
        assert!(event_when_in(&overnight, Locale::EnUs).contains(" – "));
        assert_eq!(local_range("next week\u{1b}[2J", None), "next week[2J");
    }
}
