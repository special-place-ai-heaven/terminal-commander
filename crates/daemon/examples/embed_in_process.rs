// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

use std::time::Duration;

use terminal_commanderd::{CommandStartRequest, DaemonConfig, DaemonState};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = std::env::temp_dir().join("terminal-commander-embed-example");
    let state = DaemonState::bootstrap(DaemonConfig::defaults_in(data_dir))?;

    // This is the same capability-filtered snapshot returned over IPC.
    let environment = state.discover_environment();
    println!(
        "discovered {} usable routes",
        environment.access_routes.len()
    );

    let started = state.command.start_combed(CommandStartRequest {
        argv: vec!["git".to_owned(), "--version".to_owned()],
        cwd: Some(std::env::current_dir()?),
        env: vec![],
        bucket_config: None,
        rules: vec![],
        grace: None,
        tag: Some("embed-example".to_owned()),
        strip_ansi: true,
        dedup_nonce: None,
        peer_discriminator: None,
    })?;

    tokio::time::sleep(Duration::from_millis(250)).await;
    let status = state.command.status(started.job_id)?;
    println!("job {} is {:?}", status.job_id, status.state);
    Ok(())
}
