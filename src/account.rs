use native_tls::{TlsConnector, TlsStream};

use notify_rust::{Hint, Notification};
use serde::Deserialize;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::io::prelude::*;
use std::net::TcpStream;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
pub struct Account {
    pub name: String,
    pub address: String,
    pub port: u16,
    pub username: String,
    pub password_command: String,
    pub notification_command: Option<String>,
}

impl Account {
    pub fn connect(&self) -> Result<Connection<TlsStream<TcpStream>>, imap::error::Error> {
        let tls = TlsConnector::builder().build()?;
        imap::connect((&*self.address, self.port), &self.address, &tls).and_then(|c| {
            let mut c = c
                .login(self.username.trim(), self.get_password().trim())
                .map_err(|(e, _)| e)?;
            let cap = c.capabilities()?;
            if !cap.has_str("IDLE") {
                return Err(imap::error::Error::Bad(
                    cap.iter()
                        .map(|s| format!("{:?}", s))
                        .collect::<Vec<_>>()
                        .join(","),
                ));
            }
            c.select("INBOX")?;
            Ok(Connection {
                account: self.clone(),
                socket: c,
            })
        })
    }

    fn get_password(&self) -> String {
        let mut args = self.password_command.split(' ');
        let cmd = args.next().expect("Invalid password command, it's empty!");
        let mut cmd = std::process::Command::new(cmd);
        for arg in args {
            cmd.arg(arg);
        }
        let output = &cmd.output().expect("Password command failed").stdout;
        std::str::from_utf8(output)
            .expect("Command output is invalid UTF-8")
            .into()
    }
}

pub struct Connection<T: Read + Write> {
    account: Account,
    socket: imap::Session<T>,
}

impl<T: Read + Write + imap::extensions::idle::SetReadTimeout> Connection<T> {
    pub fn handle(mut self, account: usize, mut tx: mpsc::Sender<Option<(usize, usize)>>) {
        loop {
            if let Err(e) = self.check(account, &mut tx) {
                // the connection has failed for some reason
                // try to log out (we probably can't)
                eprintln!("connection to {} failed: {:?}", self.account.name, e);
                let _ = self.socket.logout();
                break;
            }
        }

        // try to reconnect
        let mut wait = 1;
        for _ in 0..5 {
            eprintln!(
                "connection to {} lost; trying to reconnect...",
                self.account.name
            );
            match self.account.connect() {
                Ok(c) => {
                    eprintln!("{} connection reestablished", self.account.name);
                    return c.handle(account, tx);
                }
                Err(e) => {
                    eprintln!("failed to connect to {}: {:?}", self.account.name, e);
                    thread::sleep(Duration::from_secs(wait));
                }
            }

            wait *= 2;
        }
    }

    fn check(
        &mut self,
        account: usize,
        tx: &mut mpsc::Sender<Option<(usize, usize)>>,
    ) -> Result<(), imap::error::Error> {
        // Keep track of all the e-mails we have already notified about
        let mut last_notified = 0;
        let mut notification = None::<Notification>;

        loop {
            // check current state of inbox
            let mut uids = self.socket.uid_search("UNSEEN 1:*")?;
            let num_unseen = uids.len();
            if uids.iter().all(|&uid| uid <= last_notified) {
                // there are no messages we haven't already notified about
                uids.clear();
            }
            last_notified = std::cmp::max(last_notified, uids.iter().cloned().max().unwrap_or(0));

            let mut subjects = BTreeMap::new();
            if !uids.is_empty() {
                let uids: Vec<_> = uids.into_iter().map(|v: u32| format!("{}", v)).collect();
                for msg in self
                    .socket
                    .uid_fetch(&uids.join(","), "RFC822.HEADER")?
                    .iter()
                {
                    let msg = msg.header();
                    if msg.is_none() {
                        continue;
                    }

                    match mailparse::parse_headers(msg.unwrap()) {
                        Ok((headers, _)) => {
                            use mailparse::MailHeaderMap;

                            let subject = match headers.get_first_value("Subject") {
                                Some(subject) => Cow::from(subject),
                                None => Cow::from("<no subject>"),
                            };

                            let date = match headers.get_first_value("Date") {
                                Some(date) => match chrono::DateTime::parse_from_rfc2822(&date) {
                                    Ok(date) => date.with_timezone(&chrono::Local),
                                    Err(e) => {
                                        eprintln!("failed to parse message date: {:?}", e);
                                        chrono::Local::now()
                                    }
                                },
                                None => chrono::Local::now(),
                            };

                            subjects.insert(date, subject);
                        }
                        Err(e) => eprintln!("failed to parse headers of message: {:?}", e),
                    }
                }
            }

            if !subjects.is_empty() {
                if let Some(notificationcmd) = &self.account.notification_command {
                    match Command::new("sh").arg("-c").arg(notificationcmd).status() {
                        Ok(s) if s.success() => {}
                        Ok(s) => {
                            eprint!(
                                "Notification command for {} did not exit successfully.",
                                self.account.name
                            );
                            if let Some(exit_code) = s.code() {
                                eprintln!(" Exit code: {}", exit_code);
                            } else {
                                eprintln!(" Process was terminated by a signal.",);
                            }
                        }
                        Err(e) => {
                            eprintln!(
                                "Could not execute notification command for {}: {}",
                                self.account.name, e
                            );
                        }
                    }
                }

                let title = format!(
                    "@{} has new mail ({} unseen)",
                    self.account.name, num_unseen
                );

                // we want the n newest e-mail in reverse chronological order
                let mut body = String::new();
                for subject in subjects.values().rev() {
                    body.push_str("> ");
                    body.push_str(subject);
                    body.push_str("\n");
                }
                let body = body.trim_end();

                if let Some(n) = notification.take() {
                    let mut copy = Notification::new();
                    copy.clone_from(&n);
                    copy.summary(&title).body(&format!(
                        "{}",
                        askama_escape::escape(body, askama_escape::Html)
                    ));
                    copy.show().unwrap();
                    notification = Some(copy);
                } else {
                    let mut n = Notification::new();
                    n.summary(&title)
                        .body(&format!(
                            "{}",
                            askama_escape::escape(body, askama_escape::Html)
                        ))
                        .icon("notification-message-email")
                        .hint(Hint::Category("email.arrived".to_owned()))
                        .id(42); // for some reason, just updating isn't enough for dunst
                    n.show().unwrap();
                    notification = Some(n);
                }
            }

            if tx.send(Some((account, num_unseen))).is_err() {
                // we're exiting!
                break Ok(());
            }

            // IDLE until we see changes
            self.socket.idle()?.wait_keepalive()?;
        }
    }
}
