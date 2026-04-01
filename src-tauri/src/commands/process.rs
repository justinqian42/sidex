use serde::Serialize;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};

pub struct ProcessStore {
    processes: Mutex<HashMap<u32, ProcessHandle>>,
    next_id: Mutex<u32>,
}

struct ProcessHandle {
    child: Child,
    stdin: Option<std::process::ChildStdin>,
}

impl ProcessStore {
    pub fn new() -> Self {
        Self {
            processes: Mutex::new(HashMap::new()),
            next_id: Mutex::new(1),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct ProcessStdoutEvent {
    process_id: u32,
    data: String,
}

#[derive(Debug, Clone, Serialize)]
struct ProcessStderrEvent {
    process_id: u32,
    data: String,
}

#[derive(Debug, Clone, Serialize)]
struct ProcessExitEvent {
    process_id: u32,
    exit_code: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
}

#[tauri::command]
pub fn process_spawn(
    app: AppHandle,
    state: State<'_, Arc<ProcessStore>>,
    executable: String,
    args: Vec<String>,
    cwd: Option<String>,
    env: Option<HashMap<String, String>>,
) -> Result<u32, String> {
    let mut cmd = Command::new(&executable);
    cmd.args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    if let Some(env_vars) = env {
        cmd.envs(env_vars);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn '{}': {}", executable, e))?;

    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let id = {
        let mut next = state.next_id.lock().map_err(|e| e.to_string())?;
        let id = *next;
        *next += 1;
        id
    };

    {
        let mut procs = state.processes.lock().map_err(|e| e.to_string())?;
        procs.insert(id, ProcessHandle { child, stdin });
    }

    let process_id = id;
    let state_clone = state.inner().clone();

    if let Some(stdout) = stdout {
        let app_clone = app.clone();
        let pid = process_id;
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(text) => {
                        let _ = app_clone.emit(
                            "process-stdout",
                            ProcessStdoutEvent {
                                process_id: pid,
                                data: text,
                            },
                        );
                    }
                    Err(_) => break,
                }
            }
        });
    }

    if let Some(stderr) = stderr {
        let app_clone = app.clone();
        let pid = process_id;
        std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                match line {
                    Ok(text) => {
                        let _ = app_clone.emit(
                            "process-stderr",
                            ProcessStderrEvent {
                                process_id: pid,
                                data: text,
                            },
                        );
                    }
                    Err(_) => break,
                }
            }
        });
    }

    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let mut procs = match state_clone.processes.lock() {
                Ok(p) => p,
                Err(_) => return,
            };
            if let Some(handle) = procs.get_mut(&process_id) {
                match handle.child.try_wait() {
                    Ok(Some(status)) => {
                        let _ = app.emit(
                            "process-exit",
                            ProcessExitEvent {
                                process_id,
                                exit_code: status.code(),
                            },
                        );
                        procs.remove(&process_id);
                        return;
                    }
                    Ok(None) => {}
                    Err(_) => {
                        let _ = app.emit(
                            "process-exit",
                            ProcessExitEvent {
                                process_id,
                                exit_code: None,
                            },
                        );
                        procs.remove(&process_id);
                        return;
                    }
                }
            } else {
                return;
            }
        }
    });

    Ok(id)
}

#[tauri::command]
pub fn process_write(
    state: State<'_, Arc<ProcessStore>>,
    process_id: u32,
    data: String,
) -> Result<(), String> {
    let mut procs = state.processes.lock().map_err(|e| e.to_string())?;
    let handle = procs
        .get_mut(&process_id)
        .ok_or_else(|| format!("Process {} not found", process_id))?;

    if let Some(ref mut stdin) = handle.stdin {
        stdin
            .write_all(data.as_bytes())
            .map_err(|e| format!("Failed to write to process {}: {}", process_id, e))?;
        stdin
            .flush()
            .map_err(|e| format!("Failed to flush process {}: {}", process_id, e))?;
        Ok(())
    } else {
        Err(format!("Process {} stdin not available", process_id))
    }
}

#[tauri::command]
pub fn process_kill(
    state: State<'_, Arc<ProcessStore>>,
    process_id: u32,
) -> Result<(), String> {
    let mut procs = state.processes.lock().map_err(|e| e.to_string())?;
    let mut handle = procs
        .remove(&process_id)
        .ok_or_else(|| format!("Process {} not found", process_id))?;

    handle
        .child
        .kill()
        .map_err(|e| format!("Failed to kill process {}: {}", process_id, e))
}

#[tauri::command]
pub async fn process_exec(
    command: String,
    cwd: Option<String>,
    timeout: Option<u64>,
) -> Result<ProcessResult, String> {
    let result = tokio::task::spawn_blocking(move || {
        let shell = if cfg!(target_os = "windows") {
            "cmd"
        } else {
            "sh"
        };
        let flag = if cfg!(target_os = "windows") {
            "/C"
        } else {
            "-c"
        };

        let mut cmd = Command::new(shell);
        cmd.arg(flag).arg(&command);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }

        let child = cmd
            .spawn()
            .map_err(|e| format!("Failed to exec '{}': {}", command, e))?;

        if let Some(timeout_ms) = timeout {
            let (tx, rx) = std::sync::mpsc::channel();
            let handle = std::thread::spawn(move || child.wait_with_output());
            match rx.recv_timeout(std::time::Duration::from_millis(timeout_ms)) {
                Ok(_) => unreachable!(),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    drop(tx);
                    match handle.join() {
                        Ok(Ok(output)) => Ok(ProcessResult {
                            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                            exit_code: output.status.code(),
                        }),
                        Ok(Err(e)) => Err(format!("Process error: {}", e)),
                        Err(_) => Err("Process thread panicked".to_string()),
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    match handle.join() {
                        Ok(Ok(output)) => Ok(ProcessResult {
                            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                            exit_code: output.status.code(),
                        }),
                        Ok(Err(e)) => Err(format!("Process error: {}", e)),
                        Err(_) => Err("Process thread panicked".to_string()),
                    }
                }
            }
        } else {
            let output = child
                .wait_with_output()
                .map_err(|e| format!("Failed to wait for '{}': {}", command, e))?;

            Ok(ProcessResult {
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                exit_code: output.status.code(),
            })
        }
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?;

    result
}

#[tauri::command]
pub fn process_exec_sync(
    command: String,
    cwd: Option<String>,
) -> Result<ProcessResult, String> {
    let shell = if cfg!(target_os = "windows") {
        "cmd"
    } else {
        "sh"
    };
    let flag = if cfg!(target_os = "windows") {
        "/C"
    } else {
        "-c"
    };

    let mut cmd = Command::new(shell);
    cmd.arg(flag).arg(&command);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to exec '{}': {}", command, e))?;

    Ok(ProcessResult {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code(),
    })
}
