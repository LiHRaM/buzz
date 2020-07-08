#![warn(rust_2018_idioms)]

use rayon::prelude::*;

use std::fs::File;
use std::io::prelude::*;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use directories_next::ProjectDirs;

mod account;

use account::Account;

#[inline]
fn parse_failed<T>(key: &str, typename: &str) -> Option<T> {
    println!("Failed to parse '{}' as {}", key, typename);
    None
}

fn main() {
    // Load the user's config
    let config = ProjectDirs::from("", "", "buzz")
        .expect("Could not find valid home directory.")
        .config_dir()
        .with_file_name("buzz.toml");

    let config = {
        let mut f = match File::open(config) {
            Ok(f) => f,
            Err(e) => {
                println!("Could not open configuration file buzz.toml: {}", e);
                return;
            }
        };
        let mut s = String::new();
        if let Err(e) = f.read_to_string(&mut s) {
            println!("Could not read configuration file buzz.toml: {}", e);
            return;
        }
        match s.parse::<toml::Value>() {
            Ok(t) => t,
            Err(e) => {
                println!("Could not parse configuration file buzz.toml: {}", e);
                return;
            }
        }
    };

    // Figure out what accounts we have to deal with
    let accounts: Vec<_> = match config.as_table() {
        Some(t) => t
            .iter()
            .filter_map(|(name, v)| match v.as_table() {
                None => {
                    println!("Configuration for account {} is broken: not a table", name);
                    None
                }
                Some(t) => {
                    let pwcmd = match t.get("pwcmd").and_then(|p| p.as_str()) {
                        None => return None,
                        Some(pwcmd) => pwcmd,
                    };

                    let password = match Command::new("sh").arg("-c").arg(pwcmd).output() {
                        Ok(output) => String::from_utf8_lossy(&output.stdout).into_owned(),
                        Err(e) => {
                            println!("Failed to launch password command for {}: {}", name, e);
                            return None;
                        }
                    };

                    Some(Account {
                        name: name.as_str().to_owned(),
                        server: (
                            match t["server"].as_str() {
                                Some(v) => v.to_owned(),
                                None => return parse_failed("server", "string"),
                            },
                            match t["port"].as_integer() {
                                Some(v) => v as u16,
                                None => {
                                    return parse_failed("port", "integer");
                                }
                            },
                        ),
                        username: match t["username"].as_str() {
                            Some(v) => v.to_owned(),
                            None => {
                                return parse_failed("username", "string");
                            }
                        },
                        password,
                        notification_command: t.get("notificationcmd").and_then(
                            |raw_v| match raw_v.as_str() {
                                Some(v) => Some(v.to_string()),
                                None => return parse_failed("notificationcmd", "string"),
                            },
                        ),
                    })
                }
            })
            .collect(),
        None => {
            println!("Could not parse configuration file buzz.toml: not a table");
            return;
        }
    };

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

    for r in rx {
        let (i, num_unseen) = if let Some(r) = r {
            r
        } else {
            break;
        };
        unseen[i] = num_unseen;
        if unseen.iter().sum::<usize>() == 0 {
            // TODO: No new
        } else {
            // TODO: New
        }
    }
}
