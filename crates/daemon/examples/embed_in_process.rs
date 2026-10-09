// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

use std::time::Duration;

use terminal_commanderd::DaemonConfig;
use terminal_commanderd::embedded::{
    EmbeddedEngine,
    protocol::{CommandStartParams, CommandStatusParams},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = tempfile::tempdir()?;
    let engine = EmbeddedEngine::bootstrap(DaemonConfig::defaults_in(data_dir.path()))?;

    // This is the same capability-filtered snapshot returned over IPC.
    let environment = engine.system_discover().await?.environment;
    println!(
        "discovered {} usable routes",
        environment.access_routes.len()
    );

    let started = engine
        .command_start_combed(CommandStartParams::new(vec![
            "git".to_owned(),
            "--version".to_owned(),
        ]))
        .await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let status = engine
            .command_status(CommandStatusParams {
                job_id: started.job_id,
            })
            .await?;
        if status.duration_ms.is_some() {
            println!(
                "job {} is {:?}, exit {:?}",
                status.job_id, status.state, status.exit_code
            );
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            return Err("example deadline exceeded".into());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(engine.shutdown().await?.lifecycle_drained);
    Ok(())
}
