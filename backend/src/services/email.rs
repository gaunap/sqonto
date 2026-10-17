// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! Sending documents by e-mail via the user's SMTP server.

use std::time::Duration;

use anyhow::{Context, Result};
use lettre::message::header::ContentType;
use lettre::message::{Attachment, Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};

use crate::bail_user;
use crate::models::EmailSettings;

pub struct Outgoing {
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub body: String,
    /// (file name, MIME type, content)
    pub attachments: Vec<(String, String, Vec<u8>)>,
}

/// Addresses separated by comma or semicolon.
pub fn addresses(s: &str) -> Vec<String> {
    s.split([',', ';']).map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect()
}

/// The settings must be complete before anything can be sent.
pub fn check_settings(cfg: &EmailSettings) -> Result<()> {
    if cfg.host.trim().is_empty() || cfg.from_address.trim().is_empty() {
        bail_user!("Bitte in den Einstellungen unter „E-Mail“ Server und Absenderadresse angeben.");
    }
    if !(1..=65535).contains(&cfg.port) {
        bail_user!("Ungültiger Port für den E-Mail-Server.");
    }
    if !["starttls", "tls", "none"].contains(&cfg.security.as_str()) {
        bail_user!("Ungültige Verschlüsselung für den E-Mail-Server.");
    }
    Ok(())
}

fn mailbox(name: &str, address: &str) -> Result<Mailbox> {
    let address = address.trim().parse().map_err(|_| crate::error::UserError(format!("Ungültige E-Mail-Adresse „{}“.", address.trim())))?;
    Ok(Mailbox::new(if name.trim().is_empty() { None } else { Some(name.trim().to_string()) }, address))
}

pub fn build(cfg: &EmailSettings, msg: &Outgoing) -> Result<Message> {
    if msg.to.is_empty() {
        bail_user!("Bitte mindestens einen Empfänger angeben.");
    }
    let from = mailbox(&cfg.from_name, &cfg.from_address)?;
    let mut b = Message::builder().from(from.clone()).subject(msg.subject.trim());
    for to in &msg.to {
        b = b.to(mailbox("", to)?);
    }
    for cc in &msg.cc {
        b = b.cc(mailbox("", cc)?);
    }
    if cfg.bcc_self {
        b = b.bcc(from);
    }
    let mut parts = MultiPart::mixed().singlepart(SinglePart::plain(msg.body.clone()));
    for (name, mime, bytes) in &msg.attachments {
        let ct = ContentType::parse(mime).unwrap_or(ContentType::parse("application/octet-stream").expect("valid MIME type"));
        parts = parts.singlepart(Attachment::new(name.clone()).body(bytes.clone(), ct));
    }
    Ok(b.multipart(parts)?)
}

/// Sends a message (blocking – call it off the async workers).
pub fn send(cfg: &EmailSettings, msg: &Outgoing) -> Result<()> {
    check_settings(cfg)?;
    let message = build(cfg, msg)?;
    let host = cfg.host.trim();
    let builder = match cfg.security.as_str() {
        "tls" => SmtpTransport::relay(host)?,
        "starttls" => SmtpTransport::starttls_relay(host)?,
        _ => SmtpTransport::builder_dangerous(host),
    };
    let mut builder = builder.port(cfg.port as u16).timeout(Some(Duration::from_secs(30)));
    if !cfg.username.trim().is_empty() {
        builder = builder.credentials(Credentials::new(cfg.username.trim().to_string(), cfg.password.clone()));
    }
    builder
        .build()
        .send(&message)
        .map(|_| ())
        .with_context(|| format!("E-Mail konnte nicht über {host}:{} gesendet werden", cfg.port))
        .map_err(|e| crate::error::UserError(format!("{e:#}")).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> EmailSettings {
        EmailSettings { host: "smtp.example.org".into(), from_name: "Test GmbH".into(), from_address: "rechnung@example.org".into(), bcc_self: true, ..Default::default() }
    }

    #[test]
    fn messages_carry_recipients_and_attachments() {
        let msg = Outgoing {
            to: addresses("kunde@example.com; buchhaltung@example.com"),
            cc: addresses(""),
            subject: "Rechnung RE-2026-0001".into(),
            body: "Guten Tag,\n\nanbei die Rechnung.".into(),
            attachments: vec![("Rechnung_RE-2026-0001.pdf".into(), "application/pdf".into(), b"%PDF-1.7".to_vec())],
        };
        let raw = String::from_utf8(build(&cfg(), &msg).unwrap().formatted()).unwrap();
        assert!(raw.contains("To: kunde@example.com, buchhaltung@example.com"), "{raw}");
        assert!(raw.contains("From: \"Test GmbH\" <rechnung@example.org>"));
        assert!(raw.contains("Bcc: \"Test GmbH\" <rechnung@example.org>") || raw.contains("rechnung@example.org"));
        assert!(raw.contains("Rechnung_RE-2026-0001.pdf"));
        assert!(build(&cfg(), &Outgoing { to: vec!["kein-adresse".into()], cc: vec![], subject: String::new(), body: String::new(), attachments: vec![] }).is_err());
        assert!(build(&cfg(), &Outgoing { to: vec![], cc: vec![], subject: String::new(), body: String::new(), attachments: vec![] }).is_err());
    }

    #[test]
    fn incomplete_settings_are_refused() {
        assert!(check_settings(&EmailSettings::default()).is_err());
        assert!(check_settings(&cfg()).is_ok());
        assert!(check_settings(&EmailSettings { port: 0, ..cfg() }).is_err());
    }
}
