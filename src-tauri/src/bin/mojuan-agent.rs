//! Persistent local-only stdio bridge. It never opens the manuscript database.
use serde_json::{json, Value};
use std::{io, path::PathBuf};
fn main() {
    let mut socket = None;
    let mut mcp = false;
    let mut name = "local-agent".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--socket" => socket = args.next().map(PathBuf::from),
            "--mcp" => mcp = true,
            "--name" => {
                if let Some(value) = args.next() {
                    name = value
                } else {
                    fail("invalid_arguments");
                    return;
                }
            }
            _ => {
                fail("invalid_arguments");
                return;
            }
        }
    }
    let Some(socket) = socket else {
        fail("missing_socket");
        return;
    };
    if name.trim().is_empty() || name.len() > 128 {
        fail("invalid_client_name");
        return;
    }
    // Establish once, never silently reconnect/replay into a differently authorized session.
    let mut connection: Option<novel_core::agent_stdio::ConnectionGuard<novel_core::ipc::Client>> =
        None;
    let mut attempted = false;
    let result = novel_core::agent_stdio::run(
        io::stdin().lock(),
        io::stdout().lock(),
        mcp,
        |request: &Value| {
            if !attempted {
                attempted = true;
                let mut client = novel_core::ipc::connect(&socket)?;
                let access = client
                    .call(&json!({"method":"request_access","params":{"client_name":name}}))?;
                if access.get("error").is_some() {
                    return Ok(access);
                }
                connection = Some(novel_core::agent_stdio::ConnectionGuard::new(client));
            }
            match connection.as_mut() {
                Some(client) => client.call(|transport| transport.call(request)),
                None => Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "ipc_unavailable",
                )),
            }
        },
    );
    if result.is_err() {
        eprintln!("Agent stdio ended: input/output failure or oversized frame.");
        std::process::exit(1)
    }
}
fn fail(code: &str) {
    println!(
        "{}",
        json!({"error":{"code":code,"message":"Usage: mojuan-agent --socket PATH [--mcp] [--name NAME]"}})
    );
    std::process::exit(2)
}
