#![warn(rust_2018_idioms)]

use directories_next::ProjectDirs;
use rayon::prelude::*;
use std::io::Write;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

mod account;
mod waybar;
use account::Account;
use waybar::Msg;

fn main() {
    // Load the user's config
    let config = ProjectDirs::from("", "", "buzz")
        .expect("Could not find valid home directory.")
        .config_dir()
        .with_file_name("buzz.dhall");

    let accounts: Vec<Account> = serde_dhall::from_file(config).parse().unwrap();

    if accounts.is_empty() {
        println!("No accounts in config; exiting...");
        return;
    }

    let (tx, rx) = mpsc::channel();

    let accounts: Vec<_> = accounts
        .par_iter()
        .filter_map(|account| {
            let mut wait = 1;
            for _ in 0..5 {
                match account.connect() {
                    Ok(c) => return Some(c),
                    Err(imap::error::Error::Io(e)) => {
                        println!(
                            "Failed to connect account {}: {}; retrying in {}s",
                            account.name, e, wait
                        );
                        thread::sleep(Duration::from_secs(wait));
                    }
                    Err(e) => {
                        println!("{} host produced bad IMAP tunnel: {:?}", account.name, e);
                        break;
                    }
                }

                wait *= 2;
            }

            None
        })
        .collect();

    if accounts.is_empty() {
        println!("No accounts in config worked; exiting...");
        return;
    }

    let mut unseen: Vec<_> = accounts.iter().map(|_| 0).collect();
    for (i, conn) in accounts.into_iter().enumerate() {
        let tx = tx.clone();
        thread::spawn(move || {
            conn.handle(i, tx);
        });
    }

    let out = std::io::stdout();
    let mut guard = out.lock();
    for r in rx {
        let (i, num_unseen) = if let Some(r) = r {
            r
        } else {
            break;
        };
        unseen[i] = num_unseen;

        let msg = {
            let percentage = 0.0;

            if unseen.iter().sum::<usize>() == 0 {
                Msg {
                    text: "".into(),
                    tooltip: "You have reached inbox 0!".into(),
                    class: "mail-read".into(),
                    percentage,
                }
            } else {
                Msg {
                    text: "".into(),
                    tooltip: "You have unread mail!".into(),
                    class: "mail-unread".into(),
                    percentage,
                }
            }
        };

        let msg = serde_json::to_string(&msg).unwrap();

        guard.write(msg.as_bytes()).unwrap();
        guard.write(b"\n").unwrap();
        guard.flush().unwrap();
    }
}
