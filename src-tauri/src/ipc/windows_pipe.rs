use super::*;
use std::{
    fs::{File, OpenOptions},
    io::BufReader,
    os::windows::{
        ffi::OsStrExt,
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::PathBuf,
    ptr::{null, null_mut},
    sync::atomic::{AtomicBool, Ordering},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{
        LocalFree, ERROR_BROKEN_PIPE, ERROR_NO_DATA, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED,
        ERROR_PIPE_LISTENING, ERROR_PIPE_NOT_CONNECTED, INVALID_HANDLE_VALUE,
    },
    Security::{
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        },
        GetTokenInformation, TokenUser, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    },
    Storage::FileSystem::{
        ReadFile, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_READ_ATTRIBUTES, FILE_READ_DATA,
        FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA, PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION,
        SECURITY_SQOS_PRESENT, SYNCHRONIZE,
    },
    System::{
        Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, SetNamedPipeHandleState,
            PIPE_NOWAIT, PIPE_REJECT_REMOTE_CLIENTS,
        },
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
};

const PREFIX: &str = r"\\.\pipe\mojuan-";
const POLL: Duration = Duration::from_millis(12);
const ACCEPTED: usize = 16;

fn current_sid() -> io::Result<String> {
    let mut token = null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut size = 0;
    unsafe { GetTokenInformation(token.as_raw_handle(), TokenUser, null_mut(), 0, &mut size) };
    if !(std::mem::size_of::<TOKEN_USER>()..=65536).contains(&(size as usize)) {
        return Err(io::Error::last_os_error());
    }
    // TOKEN_USER and its trailing SID need pointer alignment.
    let mut storage = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            storage.as_mut_ptr().cast(),
            size,
            &mut size,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let user = unsafe { &*storage.as_ptr().cast::<TOKEN_USER>() };
    let mut sid = null_mut();
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut units = Vec::new();
    for offset in 0..256 {
        let unit = unsafe { *sid.add(offset) };
        if unit == 0 {
            break;
        }
        units.push(unit);
    }
    unsafe { LocalFree(sid.cast()) };
    String::from_utf16(&units)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid user SID"))
}

fn safe_suffix(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 220
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
}

pub fn desktop_endpoint(app_id: &str) -> io::Result<PathBuf> {
    if !safe_suffix(app_id) || app_id.len() > 120 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid app id",
        ));
    }
    Ok(PathBuf::from(format!(
        "{PREFIX}{app_id}-{}",
        current_sid()?
    )))
}

fn pipe_name(path: &Path) -> io::Result<Vec<u16>> {
    let sid = current_sid()?;
    let valid = path
        .to_str()
        .and_then(|name| name.strip_prefix(PREFIX))
        .is_some_and(|name| {
            safe_suffix(name)
                && name
                    .strip_suffix(&sid)
                    .is_some_and(|prefix| prefix.ends_with('-'))
        });
    if !valid {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected local MoJuan pipe for this user",
        ));
    }
    Ok(path.as_os_str().encode_wide().chain(Some(0)).collect())
}

fn listener(name: &[u16], descriptor: *mut core::ffi::c_void, first: bool) -> io::Result<File> {
    let security = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let flags = PIPE_ACCESS_DUPLEX
        | if first {
            FILE_FLAG_FIRST_PIPE_INSTANCE
        } else {
            0
        };
    let handle = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            flags,
            PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
            (ACCEPTED + 1) as u32,
            65536,
            65536,
            1000,
            &security,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_handle(handle) })
}

struct Descriptor(Vec<u16>);
impl Descriptor {
    fn new() -> io::Result<Self> {
        let sddl = format!("D:P(A;;GA;;;{})", current_sid()?);
        Ok(Self(sddl.encode_utf16().chain(Some(0)).collect()))
    }
    fn listener(&self, name: &[u16], first: bool) -> io::Result<File> {
        let mut descriptor = null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                self.0.as_ptr(),
                1,
                &mut descriptor,
                null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let result = listener(name, descriptor, first);
        unsafe { LocalFree(descriptor) };
        result
    }
}

struct Pipe {
    file: File,
    stop: Arc<AtomicBool>,
}

impl Pipe {
    fn retry<T>(
        &mut self,
        timeout: Duration,
        mut action: impl FnMut(&mut File) -> io::Result<Option<T>>,
    ) -> io::Result<T> {
        let started = Instant::now();
        while !self.stop.load(Ordering::Acquire) && started.elapsed() < timeout {
            match action(&mut self.file) {
                Ok(Some(value)) => return Ok(value),
                Ok(None) => {}
                Err(error) if error.raw_os_error() == Some(ERROR_NO_DATA as i32) => {}
                Err(error) => return Err(error),
            }
            thread::sleep(POLL);
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "local agent pipe timed out or closed",
        ))
    }
}

impl Read for Pipe {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        self.retry(Duration::from_secs(60), |file| {
            let mut count = 0;
            // File::read treats the empty nonblocking pipe as EOF on Windows.
            let success = unsafe {
                ReadFile(
                    file.as_raw_handle(),
                    buffer.as_mut_ptr(),
                    buffer.len().min(u32::MAX as usize) as u32,
                    &mut count,
                    null_mut(),
                )
            };
            if success != 0 {
                return Ok(Some(count as usize));
            }
            let error = io::Error::last_os_error();
            match error.raw_os_error().map(|code| code as u32) {
                Some(ERROR_BROKEN_PIPE | ERROR_PIPE_NOT_CONNECTED) => Ok(Some(0)),
                _ => Err(error),
            }
        })
    }
}
impl Write for Pipe {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        self.retry(Duration::from_secs(5), |file| {
            file.write(buffer).map(|n| (n > 0).then_some(n))
        })
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub struct Server {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.thread.take() {
            let _ = worker.join();
        }
    }
}

pub fn start_server(broker: Arc<AgentBroker>, path: &Path) -> io::Result<Server> {
    let name = pipe_name(path)?;
    let descriptor = Descriptor::new()?;
    let mut waiting = descriptor.listener(&name, true)?;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let thread = thread::spawn(move || {
        let mut workers: Vec<JoinHandle<()>> = Vec::new();
        while !flag.load(Ordering::Acquire) {
            workers.retain(|worker| !worker.is_finished());
            if workers.len() == ACCEPTED {
                thread::sleep(POLL);
                continue;
            }
            // For PIPE_NOWAIT, a nonzero result only makes a disconnected
            // instance available. A connected peer is indicated by this error.
            if unsafe { ConnectNamedPipe(waiting.as_raw_handle(), null_mut()) } != 0 {
                continue;
            }
            match io::Error::last_os_error()
                .raw_os_error()
                .map(|code| code as u32)
            {
                Some(ERROR_PIPE_CONNECTED) => {}
                Some(ERROR_PIPE_LISTENING) => {
                    thread::sleep(POLL);
                    continue;
                }
                Some(ERROR_NO_DATA | ERROR_PIPE_NOT_CONNECTED) => {
                    unsafe { DisconnectNamedPipe(waiting.as_raw_handle()) };
                    continue;
                }
                _ => break,
            }
            let Ok(next) = descriptor.listener(&name, false) else {
                break;
            };
            let connection = broker.connect();
            let file = std::mem::replace(&mut waiting, next);
            let stop = Arc::clone(&flag);
            workers.push(thread::spawn(move || {
                let pipe = Pipe { file, stop };
                let mut stream = BufReader::new(pipe);
                let Ok(connection) = connection else {
                    return;
                };
                while let Ok(Some(frame)) = read_frame(&mut stream) {
                    let response = match serde_json::from_slice::<Value>(&frame) {
                        Ok(request) => connection.call(request),
                        Err(_) => json!({"error":{"code":"invalid_json","message":"无效 JSON"}}),
                    };
                    if write_frame(stream.get_mut(), &response).is_err() {
                        break;
                    }
                }
            }));
        }
        flag.store(true, Ordering::Release);
        for worker in workers {
            let _ = worker.join();
        }
    });
    Ok(Server {
        stop,
        thread: Some(thread),
    })
}

pub struct Client(BufReader<Pipe>);

pub fn connect(path: &Path) -> io::Result<Client> {
    pipe_name(path)?;
    let started = Instant::now();
    let file = loop {
        match OpenOptions::new()
            .access_mode(
                FILE_READ_ATTRIBUTES
                    | FILE_READ_DATA
                    | FILE_WRITE_DATA
                    | FILE_WRITE_ATTRIBUTES
                    | SYNCHRONIZE,
            )
            .security_qos_flags(SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION)
            .open(path)
        {
            Ok(file) => break file,
            Err(e)
                if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32)
                    && started.elapsed() < Duration::from_secs(2) =>
            {
                thread::sleep(POLL)
            }
            Err(e) => return Err(io::Error::new(e.kind(), format!("open agent pipe: {e}"))),
        }
    };
    if unsafe { SetNamedPipeHandleState(file.as_raw_handle(), &PIPE_NOWAIT, null(), null()) } == 0 {
        let error = io::Error::last_os_error();
        return Err(io::Error::new(
            error.kind(),
            format!("configure agent pipe: {error}"),
        ));
    }
    Ok(Client(BufReader::new(Pipe {
        file,
        stop: Arc::new(AtomicBool::new(false)),
    })))
}

impl Client {
    pub fn call(&mut self, value: &Value) -> io::Result<Value> {
        if serde_json::to_vec(value)?.len() > MAX_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "frame_too_large",
            ));
        }
        write_frame(self.0.get_mut(), value)?;
        let frame = read_frame(&mut self.0)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "agent pipe closed"))?;
        serde_json::from_slice(&frame).map_err(Into::into)
    }
}
