use async_trait::async_trait;
use email::{
    account::config::{passwd::PasswdConfig, AccountConfig},
    backend::context::BackendContextBuilder,
    envelope::Id,
    imap::{
        config::{ImapAuthConfig, ImapConfig, ImapEncryptionKind},
        ImapContextBuilder, ImapContextSync,
    },
    message::{
        peek::{imap::PeekImapMessages, PeekMessages},
        Messages,
    },
    smtp::config::{SmtpAuthConfig, SmtpConfig, SmtpEncryptionKind},
    Result,
};
use mail_send::{smtp::message::IntoMessage, SmtpClient, SmtpClientBuilder};
use secret::Secret;
use std::{collections::HashSet, sync::Arc};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;

pub struct ProtonMailBridge {
    imap_context: ImapContextSync,
    smtp_client: SmtpClient<TlsStream<TcpStream>>,
}

pub struct ProtonMailBridgeBuilder {
    account_config: Arc<AccountConfig>,
    imap_config: Arc<ImapConfig>,
    smtp_config: Arc<SmtpConfig>,
}

impl ProtonMailBridgeBuilder {
    pub fn new(
        host: String,
        imap_port: u16,
        smtp_port: u16,
        user: String,
        password: String,
    ) -> Self {
        let account_config = Arc::new(AccountConfig::default());
        let passwd_config = PasswdConfig(Secret::new_raw(password));

        let imap_config = Arc::new(ImapConfig {
            host: host.to_owned(),
            port: imap_port,
            encryption: Some(ImapEncryptionKind::None),
            login: user.to_owned(),
            auth: ImapAuthConfig::Passwd(passwd_config.to_owned()),
            watch: None,
        });

        let smtp_config = Arc::new(SmtpConfig {
            host: host,
            port: smtp_port,
            encryption: Some(SmtpEncryptionKind::None),
            login: user,
            auth: SmtpAuthConfig::Passwd(passwd_config),
        });

        Self {
            account_config,
            imap_config,
            smtp_config,
        }
    }
}

impl ProtonMailBridgeBuilder {
    pub async fn build(self) -> Result<ProtonMailBridge> {
        let imap_context = ImapContextBuilder::new(self.account_config.clone(), self.imap_config)
            .build()
            .await?;

        // let smtp_context = SmtpContextBuilder::new(self.account_config, self.smtp_config.clone())
        //     .build()
        //     .await?;

        let smtp_client =
            SmtpClientBuilder::new(self.smtp_config.host.to_owned(), self.smtp_config.port)
                .credentials(self.smtp_config.credentials().await?)
                .implicit_tls(true)
                .allow_invalid_certs()
                .connect()
                .await?;

        Ok(ProtonMailBridge {
            imap_context,
            smtp_client,
        })
    }
}

#[async_trait]
impl PeekMessages for ProtonMailBridge {
    async fn peek_messages(&self, folder: &str, id: &Id) -> Result<Messages> {
        PeekImapMessages::new(&self.imap_context)
            .peek_messages(folder, id)
            .await
    }
}

// #[async_trait]
// impl SendMessage for ProtonMailBridge {
//     async fn send_message(&mutself, msg: &[u8]) -> Result<()> {
//         // SendSmtpMessage::new(&self.smtp_context)
//         //     .send_message(msg)
//         //     .await
//     }
// }

impl ProtonMailBridge {
    pub async fn search(&self, mailbox: &str, query: &str) -> Result<HashSet<u32>> {
        let guard = &mut self.imap_context.lock().await;

        guard
            .exec(
                |session| {
                    session.select(mailbox)?;
                    session.uid_search(&query)
                },
                |err| err.into(),
            )
            .await
    }

    pub async fn send_message<'x>(mut self, msg: impl IntoMessage<'x>) -> Result<()> {
        self.smtp_client.send(msg).await?;

        Ok(())
    }
}
