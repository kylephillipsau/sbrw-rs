//! Outgoing mail, for password recovery links: upstream's mail session.

use lettre::message::header::ContentTransferEncoding;
use lettre::message::{Body, Mailbox};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use sbrw_core::bo::recovery_password::RecoveryMail;

pub struct Mailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
}

impl Mailer {
    /// The SMTP server `[mail] smtp_url` names; none when it's unset or
    /// invalid (logged), so recovery mails fail as upstream's do without a
    /// mail server.
    pub fn from_url(smtp_url: Option<&str>) -> Option<Self> {
        let url = smtp_url?;
        match AsyncSmtpTransport::<Tokio1Executor>::from_url(url) {
            Ok(builder) => Some(Self { transport: builder.build() }),
            Err(err) => {
                tracing::error!("[mail] smtp_url is invalid, so no mail will be sent: {err}");
                None
            }
        }
    }

    /// `Transport.send(message)`: false, logged, on any failure, as upstream
    /// catches its `MessagingException`.
    pub async fn send(&self, mail: &RecoveryMail) -> bool {
        let message = (|| {
            let from: Mailbox = mail.from.as_deref().ok_or("no EMAIL_FROM")?.parse().map_err(|_| "bad EMAIL_FROM")?;
            let to: Mailbox = mail.to.as_deref().ok_or("no recipient")?.parse().map_err(|_| "bad recipient")?;
            Message::builder()
                .from(from)
                .to(to)
                .subject(mail.subject)
                .body(body(&mail.text))
                .map_err(|_| "unbuildable message")
        })();
        let result = match message {
            Ok(message) => self.transport.send(message).await.map(drop).map_err(|err| err.to_string()),
            Err(err) => Err(err.to_owned()),
        };
        if let Err(err) = &result {
            tracing::warn!(to = ?mail.to, "recovery mail not sent: {err}");
        }
        result.is_ok()
    }
}

/// JavaMail's choice for `setText`: 7bit for ASCII text with no line over
/// RFC 5322's 998 characters, as the recovery mail is, so the link reaches
/// readers unencoded. (lettre would quote any line over 76.)
fn body(text: &str) -> Body {
    let seven_bit = text.is_ascii() && !text.contains('\0') && text.lines().all(|line| line.len() <= 998);
    if seven_bit {
        let crlf = text.replace("\r\n", "\n").replace('\n', "\r\n");
        Body::dangerous_pre_encoded(crlf.into_bytes(), ContentTransferEncoding::SevenBit)
    } else {
        Body::new(text.to_owned())
    }
}
