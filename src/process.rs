//! File-backed probe execution shares one deadline and child-reaping policy.
use anyhow::Result;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

pub(crate) struct ProcessExit {
    pub status: ExitStatus,
    pub timed_out: bool,
}

pub(crate) fn run(command: &mut Command, timeout: Duration) -> Result<ProcessExit> {
    command.stdin(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn()?;
    let result = wait(&mut child, timeout);
    if let Ok(Some(status)) = result {
        return Ok(ProcessExit {
            status,
            timed_out: false,
        });
    }
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{}", child.id())])
            .status();
    }
    let _ = child.kill();
    let status = child.wait();
    result?;
    Ok(ProcessExit {
        status: status?,
        timed_out: true,
    })
}

fn wait(child: &mut Child, timeout: Duration) -> Result<Option<ExitStatus>> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if start.elapsed() >= timeout {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(crate) fn text(command: &mut Command) -> Option<String> {
    let output = command.output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
