use serde_json::{json, Value};
use std::io::{self, Read, Write};

const ORIGIN: &str = "chrome-extension://kbckhlgpbooehckhepcdcoknakpkalce/";

fn run(
    mut input: impl Read,
    mut output: impl Write,
    mut call: impl FnMut(&Value) -> io::Result<Value>,
) -> io::Result<()> {
    let mut header = [0; 4];
    while input.read(&mut header[..1])? == 1 {
        input.read_exact(&mut header[1..])?;
        let size = u32::from_le_bytes(header) as usize;
        if size > 16_384 {
            return Err(io::Error::other("browser request too large"));
        }
        let mut data = vec![0; size];
        input.read_exact(&mut data)?;
        let msg: Value = serde_json::from_slice(&data)?;
        let method = msg["method"].as_str().unwrap_or("");
        let reply = if matches!(
            method,
            "session_status"
                | "browser_bind"
                | "browser_volumes"
                | "browser_claim"
                | "browser_validate"
                | "browser_body_chunk"
                | "browser_finish"
        ) {
            call(
                &json!({"method":method,"params":msg.get("params").cloned().unwrap_or(json!({}))}),
            )?
        } else {
            json!({"error":{"message":"浏览器请求无效"}})
        };
        let frame = serde_json::to_vec(&json!({"id":msg["id"],"reply":reply}))?;
        if frame.len() > 1_048_576 {
            return Err(io::Error::other("browser response too large"));
        }
        output.write_all(&(frame.len() as u32).to_le_bytes())?;
        output.write_all(&frame)?;
        output.flush()?;
    }
    Ok(())
}
#[cfg(windows)]
fn main() -> io::Result<()> {
    if std::env::args().nth(1).as_deref() != Some(ORIGIN) {
        return Err(io::Error::other("unexpected browser origin"));
    }
    let path = novel_core::ipc::desktop_endpoint("com.mojuan.writer")?;
    let mut pipe = novel_core::ipc::connect(&path)?;
    let pending = pipe.call(
        &json!({"method":"request_browser_access","params":{"client_name":"墨卷番茄扩展"}}),
    )?;
    if pending["result"]["status"] != "pending" {
        return Err(io::Error::other("browser access unavailable"));
    }
    run(io::stdin().lock(), io::stdout().lock(), |m| pipe.call(m))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_frames_preserve_ids_and_only_forward_browser_methods() {
        let mut input = Vec::new();
        for msg in [
            json!({"id":1,"method":"browser_claim"}),
            json!({"id":2,"method":"propose_edit"}),
        ] {
            let bytes = serde_json::to_vec(&msg).unwrap();
            input.extend((bytes.len() as u32).to_le_bytes());
            input.extend(bytes);
        }
        let mut output = Vec::new();
        run(&input[..], &mut output, |q| {
            assert_eq!(q, &json!({"method":"browser_claim","params":{}}));
            Ok(json!({"result":{"body":"段落🙂\n第二段"}}))
        })
        .unwrap();
        let mut reader = &output[..];
        for id in [1, 2] {
            let mut header = [0; 4];
            reader.read_exact(&mut header).unwrap();
            let mut bytes = vec![0; u32::from_le_bytes(header) as usize];
            reader.read_exact(&mut bytes).unwrap();
            let parsed: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(parsed["id"], id);
            if id == 1 {
                assert_eq!(parsed["reply"]["result"]["body"], "段落🙂\n第二段");
            } else {
                assert!(parsed["reply"]["error"].is_object());
            }
        }
        assert!(reader.is_empty());
        assert!(run(&[255; 4][..], Vec::new(), |_| panic!()).is_err());
    }
}
#[cfg(not(windows))]
fn main() {
    eprintln!("Windows desktop required");
}
