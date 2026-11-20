//! Bounded, local-only IPC for the author-authorized agent protocol.
use crate::agent::AgentBroker;
use serde_json::{json, Value};
use std::{
    io::{self, Read, Write},
    path::Path,
    sync::Arc,
};
pub const MAX_FRAME: usize = 2 * 1024 * 1024;
#[cfg(not(windows))]
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "agent_ipc_unavailable: 此平台尚无经验证的安全本机 IPC；不会退回 TCP",
    )
}
pub fn read_frame<R: Read>(reader: &mut R) -> io::Result<Option<Vec<u8>>> {
    let deadline = std::time::Instant::now();
    let mut frame = Vec::new();
    let mut byte = [0];
    loop {
        if deadline.elapsed().as_secs() >= 60 {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "frame timeout"));
        }
        match reader.read(&mut byte) {
            Ok(0) => {
                return if frame.is_empty() {
                    Ok(None)
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "incomplete frame",
                    ))
                }
            }
            Ok(_) => {
                if byte[0] == b'\n' {
                    return Ok(Some(frame));
                }
                if frame.len() >= MAX_FRAME {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "frame_too_large",
                    ));
                }
                frame.push(byte[0]);
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
}
fn write_frame<W: Write>(writer: &mut W, v: &Value) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(v)?;
    if bytes.len() > MAX_FRAME {
        bytes = serde_json::to_vec(
            &json!({"error":{"code":"limit_exceeded","message":"响应超过 2 MiB，请缩小读取内容"}}),
        )?;
    }
    writer.write_all(&bytes)?;
    writer.write_all(b"\n")?;
    writer.flush()
}
#[cfg(unix)]
mod platform {
    use super::*;
    use std::{
        fs,
        os::unix::{
            fs::{DirBuilderExt, MetadataExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
        path::{Component, PathBuf},
        sync::{
            atomic::{AtomicBool, Ordering},
            Mutex,
        },
        thread::{self, JoinHandle},
        time::Duration,
    };
    fn validate_ancestors(path: &Path) -> io::Result<()> {
        if !path.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "IPC path must be absolute",
            ));
        }
        let mut walk = PathBuf::new();
        for component in path.components() {
            if matches!(component, Component::ParentDir) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "parent traversal denied",
                ));
            }
            walk.push(component.as_os_str());
            let m = fs::symlink_metadata(&walk)?;
            if m.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "symlink denied",
                ));
            }
            let uid = unsafe { libc::geteuid() };
            let mode = m.permissions().mode();
            // An attacker-owned or non-sticky shared ancestor can replace the private directory.
            if !m.is_dir()
                || (m.uid() != 0 && m.uid() != uid)
                || (mode & 0o022 != 0 && mode & 0o1000 == 0)
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "unsafe IPC ancestor",
                ));
            }
        }
        Ok(())
    }
    fn validate_dir(path: &Path) -> io::Result<()> {
        validate_ancestors(path)?;
        let m = fs::symlink_metadata(path)?;
        let uid = unsafe { libc::geteuid() };
        if !m.is_dir() || m.uid() != uid || m.permissions().mode() & 0o777 != 0o700 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "IPC directory must be owner-only mode 0700",
            ));
        }
        Ok(())
    }
    fn validate_socket(path: &Path) -> io::Result<()> {
        use std::os::unix::fs::FileTypeExt;
        validate_dir(path.parent().ok_or_else(unsupported)?)?;
        let m = fs::symlink_metadata(path)?;
        if !m.file_type().is_socket()
            || m.uid() != unsafe { libc::geteuid() }
            || m.permissions().mode() & 0o777 != 0o600
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "IPC socket must be owner-only mode 0600",
            ));
        }
        Ok(())
    }
    pub struct Server {
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
        path: PathBuf,
        sockets: Arc<Mutex<std::collections::HashMap<String, UnixStream>>>,
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Ok(sockets) = self.sockets.lock() {
                for socket in sockets.values() {
                    let _ = socket.shutdown(std::net::Shutdown::Both);
                }
            }
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
            let _ = fs::remove_file(&self.path);
        }
    }
    pub fn start_server(broker: Arc<AgentBroker>, path: &Path) -> io::Result<Server> {
        let dir = path.parent().ok_or_else(unsupported)?;
        if !dir.exists() {
            validate_ancestors(dir.parent().ok_or_else(unsupported)?)?;
            fs::DirBuilder::new().mode(0o700).create(dir)?;
        }
        validate_dir(dir)?;
        if fs::symlink_metadata(path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "IPC socket path already exists; refusing replacement",
            ));
        }
        let listener = UnixListener::bind(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let sockets = Arc::new(Mutex::new(
            std::collections::HashMap::<String, UnixStream>::new(),
        ));
        let active = sockets.clone();
        let thread = thread::spawn(move || {
            let mut workers: Vec<JoinHandle<()>> = Vec::new();
            while !flag.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if stream
                            .set_read_timeout(Some(Duration::from_secs(60)))
                            .is_err()
                            || stream
                                .set_write_timeout(Some(Duration::from_secs(5)))
                                .is_err()
                        {
                            continue;
                        }
                        let connection = match broker.connect() {
                            Ok(c) => c,
                            Err(_) => {
                                let _ = write_frame(
                                    &mut stream,
                                    &json!({"error":{"code":"limit_exceeded","message":"连接数已满"}}),
                                );
                                continue;
                            }
                        };
                        let flag = flag.clone();
                        let id = connection.id().to_string();
                        let clone = match stream.try_clone() {
                            Ok(s) => s,
                            Err(_) => continue,
                        };
                        active.lock().unwrap().insert(id.clone(), clone);
                        let active = active.clone();
                        let reader = match stream.try_clone() {
                            Ok(s) => s,
                            Err(_) => {
                                active.lock().unwrap().remove(&id);
                                continue;
                            }
                        };
                        workers.push(thread::spawn(move||{let mut reader=std::io::BufReader::new(reader);while !flag.load(Ordering::Acquire){let frame=match read_frame(&mut reader){Ok(Some(v))=>v,_=>break};let response=match serde_json::from_slice::<Value>(&frame){Ok(v)=>connection.call(v),Err(_)=>json!({"error":{"code":"invalid_json","message":"无效 JSON"}})};if write_frame(&mut stream,&response).is_err(){break}}active.lock().unwrap().remove(&id);}));
                        workers.retain(|w| !w.is_finished());
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10))
                    }
                    Err(_) => break,
                }
            }
            for worker in workers {
                let _ = worker.join();
            }
        });
        Ok(Server {
            stop,
            thread: Some(thread),
            path: path.into(),
            sockets,
        })
    }
    pub struct Client {
        stream: UnixStream,
        reader: std::io::BufReader<UnixStream>,
    }
    impl Client {
        pub fn call(&mut self, v: &Value) -> io::Result<Value> {
            let data = serde_json::to_vec(v)?;
            if data.len() > MAX_FRAME {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "frame_too_large",
                ));
            }
            write_frame(&mut self.stream, v)?;
            let frame = read_frame(&mut self.reader)?
                .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "IPC closed"))?;
            Ok(serde_json::from_slice(&frame)?)
        }
    }
    pub fn connect(path: &Path) -> io::Result<Client> {
        validate_socket(path)?;
        let stream = UnixStream::connect(path)?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;
        let reader = std::io::BufReader::new(stream.try_clone()?);
        Ok(Client { stream, reader })
    }
}
#[cfg(unix)]
pub use platform::*;
#[cfg(windows)]
mod windows_pipe;
#[cfg(windows)]
pub use windows_pipe::*;
#[cfg(not(any(unix, windows)))]
pub struct Server;
#[cfg(not(any(unix, windows)))]
pub struct Client;
#[cfg(not(any(unix, windows)))]
pub fn start_server(_: Arc<AgentBroker>, _: &Path) -> io::Result<Server> {
    Err(unsupported())
}
#[cfg(not(any(unix, windows)))]
pub fn connect(_: &Path) -> io::Result<Client> {
    Err(unsupported())
}
#[cfg(not(any(unix, windows)))]
impl Client {
    pub fn call(&mut self, _: &Value) -> io::Result<Value> {
        Err(unsupported())
    }
}
