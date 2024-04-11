use chrono::Datelike;
use dotenvy::dotenv;
use email::{
    envelope::Id,
    folder,
    message::{peek::PeekMessages, send::SendMessage, Attachment},
    Result,
};
use mail_send::mail_builder::MessageBuilder;
use std::{env, io};
use tokio;

mod proton_mail_bridge;

use crate::proton_mail_bridge::{ProtonMailBridge, ProtonMailBridgeBuilder};

use log::LevelFilter;

async fn fetch_bank_statements_for_previous_month(
    bridge: &ProtonMailBridge,
) -> Result<Vec<Attachment>> {
    let mut results = Vec::<Attachment>::new();

    let first_day_of_current_month = chrono::Utc::now().with_day(1).unwrap();
    let search_query = format!(
        "FROM kontakt@mbank.pl SUBJECT \"elektroniczne zestawienie operacji za\" SINCE {}",
        first_day_of_current_month.format("%d-%b-%Y")
    );
    println!("{}", search_query);

    let mailboxes_to_search = [folder::INBOX, folder::TRASH];

    for mailbox in mailboxes_to_search {
        let message_uids = bridge.search(mailbox, &search_query).await?;

        if message_uids.is_empty() {
            continue;
        }

        let message_uid_set = Id::multiple(message_uids);
        let messages = bridge.peek_messages(mailbox, &message_uid_set).await?;
        let messages_vector = messages.to_vec();

        let bank_statements = messages_vector
            .iter()
            .flat_map(|message| message.attachments().unwrap())
            .filter(|attachment| attachment.filename.as_ref().unwrap().starts_with("mBiznes"));

        results.extend(bank_statements);
    }

    Ok(results)
}

async fn send_files(
    bridge: ProtonMailBridge,
    from_address: &str,
    recipient_address: &str,
    attachments: Vec<Attachment>,
) -> Result<()> {
    let message = MessageBuilder::new()
        .from(from_address)
        .to(recipient_address)
        .subject("Wyciągi");

    let message_with_attachments = attachments.into_iter().fold(message, |acc, attachment| {
        acc.attachment(
            attachment.mime,
            attachment.filename.unwrap(),
            attachment.body,
        )
    });

    let message_bytes = message_with_attachments.write_to_vec().unwrap();

    bridge.send_message(&message_bytes).await
}

#[tokio::main]
async fn main() {
    env_logger::builder()
        .filter_level(LevelFilter::Debug)
        .init();

    dotenv().unwrap();

    let host = env::var("ACCOUNTING_MAIL_HOST").unwrap();
    let user = env::var("ACCOUNTING_MAIL_USER").unwrap();
    let password = env::var("ACCOUNTING_MAIL_PASSWORD").unwrap();
    let imap_port = env::var("ACCOUNTING_IMAP_PORT")
        .unwrap()
        .parse::<u16>()
        .unwrap();
    let smtp_port = env::var("ACCOUNTING_SMTP_PORT")
        .unwrap()
        .parse::<u16>()
        .unwrap();
    let invoice_recipient = env::var("ACCOUNTING_INVOICE_RECIPIENT").unwrap();

    let bridge_builder = ProtonMailBridgeBuilder::new(
        host.clone(),
        imap_port,
        smtp_port,
        user.clone(),
        password.clone(),
    );

    let bridge = bridge_builder.build().await.unwrap();

    let bank_statements = fetch_bank_statements_for_previous_month(&bridge)
        .await
        .unwrap();

    for bank_statement in &bank_statements {
        println!("Found: {}", bank_statement.filename.as_ref().unwrap());
    }

    if bank_statements.is_empty() {
        println!("Nothing to do, exiting");
        return;
    }

    println!("Send to {invoice_recipient}?");

    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();

    if input.to_lowercase().starts_with("y") {
        println!("Sending");

        send_files(bridge, &user, &invoice_recipient, bank_statements)
            .await
            .unwrap();
    }
}
