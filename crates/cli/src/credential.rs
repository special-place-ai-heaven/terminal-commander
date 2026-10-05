// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 The Terminal Commander Authors

//! `credential provide <job_id>`: the owner answers a PTY job's password
//! prompt from their own terminal. The password is read with echo off, sent
//! once over the local socket, and typed into that job by the daemon. The
//! daemon accepts it only from this CLI image, never from the MCP adapter.

// Same convention as `ipc.rs`: explicit crate visibility on a private module.
#![allow(clippy::redundant_pub_crate)]

use std::process::ExitCode;

use terminal_commander_ipc::{
    CredentialProvideParams, IpcRequest, IpcResponse, OwnerSecret, owner_prompt_text,
};

use crate::ipc::connect_or_unavailable_at;

/// `socket`: the daemon named in `credential_request`'s command; the owner's
/// terminal does not carry the harness's `TC_SESSION`.
pub(crate) fn run_provide(job_id: &str, socket: Option<&std::path::Path>) -> ExitCode {
    let endpoint = socket.map_or_else(
        terminal_commander_supervisor::paths::resolve_socket_path,
        std::path::Path::to_path_buf,
    );
    let job_id = match terminal_commander_core::JobId::parse_wire(job_id) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("terminal-commander: credential provide: invalid job id {job_id:?}: {e}");
            return ExitCode::from(2);
        }
    };
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("terminal-commander: tokio runtime build failed: {e}");
            return ExitCode::from(2);
        }
    };

    // Show WHICH job asks before reading anything: a job the model started
    // can print a fake prompt, and the owner decides whether to answer it.
    let entries = match rt.block_on(connect_or_unavailable_at(
        1,
        IpcRequest::PtyCommandList,
        &endpoint,
    )) {
        Ok(IpcResponse::PtyCommandList(r)) => r.entries,
        Ok(_) => {
            eprintln!("terminal-commander: credential provide: unexpected daemon response");
            return ExitCode::from(1);
        }
        Err(err) => {
            err.report("credential provide");
            return ExitCode::from(err.exit_code());
        }
    };
    let Some(entry) = entries.iter().find(|e| e.job_id == job_id) else {
        eprintln!(
            "terminal-commander: credential provide: pty job {} is not live",
            job_id.to_wire_string()
        );
        return ExitCode::from(1);
    };
    let Some(awaiting) = entry.awaiting_credential else {
        eprintln!(
            "terminal-commander: credential provide: pty job {} is not waiting for a password",
            job_id.to_wire_string()
        );
        return ExitCode::from(1);
    };
    // The same text the native dialog and the loopback page show.
    eprintln!(
        "{}",
        owner_prompt_text(
            job_id,
            awaiting.kind,
            entry.program.as_deref().unwrap_or("(unknown)"),
            &entry.argv,
            &entry.program_env,
        )
    );

    let (secret, interactive) = match read_secret() {
        Ok((s, tty)) => (OwnerSecret::new(s), tty),
        Err(e) => {
            eprintln!("terminal-commander: credential provide: could not read the password: {e}");
            return ExitCode::from(1);
        }
    };
    let request = IpcRequest::CredentialProvide(CredentialProvideParams {
        job_id,
        secret,
        from_mcp: false,
        interactive,
    });
    match rt.block_on(connect_or_unavailable_at(2, request, &endpoint)) {
        Ok(IpcResponse::CredentialProvide(_)) => {
            println!("provided");
            ExitCode::SUCCESS
        }
        Ok(_) => {
            eprintln!("terminal-commander: credential provide: unexpected daemon response");
            ExitCode::from(1)
        }
        Err(err) => {
            err.report("credential provide");
            ExitCode::from(err.exit_code())
        }
    }
}

/// One line from stdin, echo off when stdin is a terminal. Piped stdin is
/// read as-is (scripted owners, tests); the flag tells the daemon which it
/// was, so the audit row can say `cli-tty` or `cli-stdin`.
fn read_secret() -> std::io::Result<(String, bool)> {
    use std::io::{BufRead, IsTerminal, Write};

    let stdin = std::io::stdin();
    let tty = stdin.is_terminal();
    let mut line = String::new();
    {
        let _echo = if tty { Some(EchoOff::new()?) } else { None };
        eprint!("Password: ");
        std::io::stderr().flush()?;
        stdin.lock().read_line(&mut line)?;
    }
    if tty {
        eprintln!();
    }
    while line.ends_with(['\n', '\r']) {
        line.pop();
    }
    Ok((line, tty))
}

/// Terminal echo off for the guard's lifetime.
#[cfg(unix)]
struct EchoOff;

#[cfg(unix)]
impl EchoOff {
    // ponytail: `stty` over termios FFI; a Ctrl-C mid-read leaves echo off
    // (`stty echo` restores).
    fn new() -> std::io::Result<Self> {
        stty("-echo")?;
        Ok(Self)
    }
}

#[cfg(unix)]
impl Drop for EchoOff {
    fn drop(&mut self) {
        let _ = stty("echo");
    }
}

#[cfg(unix)]
fn stty(arg: &str) -> std::io::Result<()> {
    let status = std::process::Command::new("stty")
        .arg(arg)
        .stdin(std::process::Stdio::inherit())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!("stty {arg} failed")))
    }
}

#[cfg(windows)]
struct EchoOff {
    handle: windows::Win32::Foundation::HANDLE,
    mode: windows::Win32::System::Console::CONSOLE_MODE,
}

#[cfg(windows)]
impl EchoOff {
    fn new() -> std::io::Result<Self> {
        use windows::Win32::System::Console::{
            CONSOLE_MODE, ENABLE_ECHO_INPUT, GetConsoleMode, GetStdHandle, STD_INPUT_HANDLE,
            SetConsoleMode,
        };
        // SAFETY: plain console-handle queries on this process's own stdin;
        // `mode` is a live out-parameter.
        unsafe {
            let handle = GetStdHandle(STD_INPUT_HANDLE).map_err(std::io::Error::other)?;
            let mut mode = CONSOLE_MODE::default();
            GetConsoleMode(handle, &raw mut mode).map_err(std::io::Error::other)?;
            SetConsoleMode(handle, CONSOLE_MODE(mode.0 & !ENABLE_ECHO_INPUT.0))
                .map_err(std::io::Error::other)?;
            Ok(Self { handle, mode })
        }
    }
}

#[cfg(windows)]
impl Drop for EchoOff {
    fn drop(&mut self) {
        // SAFETY: restores the mode read in `new` on the same handle.
        unsafe {
            let _ = windows::Win32::System::Console::SetConsoleMode(self.handle, self.mode);
        }
    }
}

#[cfg(not(any(unix, windows)))]
struct EchoOff;

#[cfg(not(any(unix, windows)))]
impl EchoOff {
    fn new() -> std::io::Result<Self> {
        Err(std::io::Error::other(
            "echo-off input is not supported on this platform",
        ))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn argv_control_characters_cannot_reach_the_owner_terminal() {
        // The CLI prints the shared owner text, which sanitizes argv.
        let text = terminal_commander_ipc::owner_prompt_text(
            terminal_commander_core::JobId::new(),
            terminal_commander_ipc::CredentialKind::Sudo,
            "/usr/bin/sudo",
            &["sudo".to_owned(), "\u{1b}]0;x\u{7}ls\u{202e}".to_owned()],
            &[],
        );
        assert!(text.contains("Command: sudo ?]0;x?ls?"), "{text}");
    }
}
