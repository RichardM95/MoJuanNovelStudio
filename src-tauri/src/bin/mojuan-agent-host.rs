//! Linux development host: explicit authorization requires an interactive terminal.
use novel_core::{agent::AgentBroker, Service};
use std::{
    io::{self, IsTerminal, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
};
fn main() {
    if let Err(error) = run() {
        eprintln!("Host stopped: {error}");
        std::process::exit(1)
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    if !io::stdin().is_terminal() {
        return Err("Interactive terminal required; piped grants are not accepted".into());
    }
    let mut database = None;
    let mut socket = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--database" => database = args.next().map(PathBuf::from),
            "--socket" => socket = args.next().map(PathBuf::from),
            _ => return Err("Usage: mojuan-agent-host --database PATH --socket PATH".into()),
        }
    }
    let database = database.ok_or("Missing --database")?;
    let socket = socket.ok_or("Missing --socket")?;
    let service = Arc::new(Mutex::new(
        Service::open(database).map_err(io::Error::other)?,
    ));
    let broker = Arc::new(AgentBroker::new(service.clone()));
    let _server = novel_core::ipc::start_server(broker.clone(), &socket)?;
    eprintln!("Local agent host. Commands: sessions | books | grant SESSION BOOK | revoke SESSION | quit. Client names are unverified labels. Grant only a connection you expect.");
    let stdin = io::stdin();
    let mut input = stdin.lock();
    loop {
        print!("host> ");
        io::stdout().flush()?;
        let Some(line) = novel_core::agent_stdio::read_frame(&mut input)? else {
            break;
        };
        let line = String::from_utf8(line)?;
        let words: Vec<_> = line.split_whitespace().collect();
        match words.as_slice() {
            ["quit"] => break,
            ["sessions"] => println!("{}", serde_json::to_string(&broker.list_sessions())?),
            ["books"] => {
                let store = service.lock().map_err(|_| "Service lock unavailable")?;
                let state = store.load().map_err(io::Error::other)?;
                println!(
                    "{}",
                    serde_json::to_string(
                        &state
                            .books
                            .into_iter()
                            .filter(|b| b.deleted_at.is_none())
                            .collect::<Vec<_>>()
                    )?
                );
            }
            ["grant", session, book] => {
                let label = broker
                    .list_sessions()
                    .into_iter()
                    .find(|s| s.id == *session)
                    .map(|s| s.client_name);
                let title = service
                    .lock()
                    .map_err(|_| "Service lock unavailable")?
                    .get_outline(book)
                    .ok()
                    .map(|b| b.book.title);
                println!(
                    "{}",
                    serde_json::json!({"granting_session":session,"unverified_client_name":label,"book_id":book,"book_title":title})
                );
                match broker.grant_session(session, book) {
                    Ok(_) => println!("Granted"),
                    Err(e) => println!("{}", serde_json::to_string(&e)?),
                }
            }
            ["revoke", session] => match broker.revoke_session(session) {
                Ok(_) => println!("Revoked"),
                Err(e) => println!("{}", serde_json::to_string(&e)?),
            },
            [] => (),
            _ => println!("Unknown command"),
        }
    }
    Ok(())
}
