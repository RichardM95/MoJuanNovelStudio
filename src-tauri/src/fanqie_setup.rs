use crate::desktop::DesktopAgents;
use std::{
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
};
use tauri::{AppHandle, Manager, State};

fn helper(name: &str) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let path = exe.with_file_name(name);
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!("找不到 {name}，请重新构建墨卷连接组件"))
    }
}
fn invoke(executable: &Path, args: &[&std::ffi::OsStr]) -> Result<(), String> {
    let result = Command::new(executable)
        .args(args)
        .creation_flags(0x08000000)
        .output()
        .map_err(|e| e.to_string())?;
    if result.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&result.stderr).trim().to_owned())
    }
}
#[tauri::command]
pub(crate) async fn connect_codex(state: State<'_, DesktopAgents>) -> Result<String, String> {
    let endpoint = state.endpoint.clone().ok_or("本机接口未启动")?;
    let agent = helper("mojuan-agent.exe")?;
    let codex = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .filter(|p| p.is_absolute())
        .map(|p| p.join("codex.exe"))
        .find(|p| p.is_file())
        .ok_or("找不到 codex.exe，请将 Codex CLI 加入 PATH")?;
    tauri::async_runtime::spawn_blocking(move || {
        invoke(
            &codex,
            &[
                "mcp".as_ref(),
                "add".as_ref(),
                "mojuan".as_ref(),
                "--".as_ref(),
                agent.as_os_str(),
                "--socket".as_ref(),
                endpoint.as_ref(),
                "--mcp".as_ref(),
                "--name".as_ref(),
                "Codex".as_ref(),
            ],
        )
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok("已配置墨卷 MCP。在 Codex 中刷新工具或新开对话，调用 session_status 后在这里授权。".into())
}
#[tauri::command]
pub(crate) async fn register_fanqie_browser(
    app: AppHandle,
    state: State<'_, DesktopAgents>,
) -> Result<String, String> {
    let host = helper("mojuan-fanqie-host.exe")?;
    let manifest = state.data_dir.join("fanqie-native-host.json");
    let config = serde_json::json!({"name":"com.mojuan.fanqie","description":"墨卷番茄草稿连接",
        "path":host,"type":"stdio","allowed_origins":["chrome-extension://kbckhlgpbooehckhepcdcoknakpkalce/"]});
    std::fs::write(&manifest, config.to_string()).map_err(|e| e.to_string())?;
    let reg = PathBuf::from(std::env::var_os("SystemRoot").ok_or("无法定位 Windows")?)
        .join("System32/reg.exe");
    tauri::async_runtime::spawn_blocking(move || {
        for browser in [r"Google\Chrome", r"Microsoft\Edge"] {
            let key = format!(r"HKCU\Software\{browser}\NativeMessagingHosts\com.mojuan.fanqie");
            invoke(
                &reg,
                &[
                    "add".as_ref(),
                    key.as_ref(),
                    "/ve".as_ref(),
                    "/t".as_ref(),
                    "REG_SZ".as_ref(),
                    "/d".as_ref(),
                    manifest.as_os_str(),
                    "/f".as_ref(),
                ],
            )?;
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| e.to_string())??;
    let bundled = app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?
        .join("fanqie-extension");
    let extension = if bundled.is_dir() {
        bundled
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../tools/fanqie-browser-extension/extension")
    };
    Ok(format!(
        "在 Chrome 或 Edge 加载已解压的扩展：{}。打开番茄草稿箱，从扩展连接墨卷。",
        extension.display()
    ))
}
