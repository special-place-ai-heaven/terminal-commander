// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

use terminal_commanderd::embedded::{
    EmbeddedEngine, IsolatedCommand,
    protocol::{CommandStartParams, CommandStatusParams},
};
use terminal_commanderd::{DaemonConfig, PolicyProfile};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The same executable is a portable child fixture; it reports no env values.
    if std::env::var_os("TC_EXAMPLE_CHILD").is_some() {
        assert!(std::env::var_os("PATH").is_none());
        println!("isolated child completed");
        return Ok(());
    }
    let room = tempfile::tempdir()?;
    let mut config = DaemonConfig::defaults_in(room.path().join("tc-data"));
    config.policy.profile = PolicyProfile::RepoOnly;
    config.policy.repo_root = Some(room.path().to_path_buf());
    let engine = EmbeddedEngine::bootstrap(config)?;
    let mut command = CommandStartParams::new(vec![
        std::env::current_exe()?.to_string_lossy().into_owned(),
    ]);
    command
        .env
        .push(("TC_EXAMPLE_CHILD".to_owned(), "present".to_owned()));
    let started = engine
        .command_start_isolated(IsolatedCommand::new(command, room.path().to_path_buf()))
        .await?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let status = engine
            .command_status(CommandStatusParams {
                job_id: started.job_id,
            })
            .await?;
        if status.duration_ms.is_some() {
            assert_eq!(status.exit_code, Some(0));
            assert!(
                status
                    .process_observation
                    .is_some_and(|observation| observation.is_complete())
            );
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            return Err("example deadline exceeded".into());
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let report = engine.shutdown().await?;
    assert!(report.store_closed && report.lifecycle_drained);
    Ok(())
}
