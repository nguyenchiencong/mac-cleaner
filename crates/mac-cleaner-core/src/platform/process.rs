use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use thiserror::Error;

/// Maximum bytes of stdout/stderr captured to prevent unbounded memory usage.
pub const MAX_PROCESS_OUTPUT_BYTES: usize = 1024 * 1024; // 1 MB

#[derive(Error, Debug)]
pub enum ProcessError {
    #[error("Command not found: {0}")]
    CommandNotFound(String),

    #[error("Command timed out after {0:?}")]
    Timeout(Duration),

    #[error("Failed to execute command '{command}': {details}")]
    ExecutionFailed { command: String, details: String },

    #[error("I/O error during process execution: {0}")]
    Io(#[from] std::io::Error),
}

/// Output captured from a direct process invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    pub exit_code: Option<i32>,
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
}

/// Direct process executor invoking tools with structured arguments (no shell interpolation).
#[derive(Debug, Default, Clone)]
pub struct ProcessExecutor;

impl ProcessExecutor {
    pub fn new() -> Self {
        Self
    }

    /// Executes a command with structured arguments and optional timeout.
    pub fn execute(
        &self,
        program: &str,
        args: &[&str],
        timeout: Option<Duration>,
    ) -> Result<ProcessOutput, ProcessError> {
        let start_time = Instant::now();

        let mut child = Command::new(program)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| {
                if err.kind() == std::io::ErrorKind::NotFound {
                    ProcessError::CommandNotFound(program.to_string())
                } else {
                    ProcessError::ExecutionFailed {
                        command: format!("{} {}", program, args.join(" ")),
                        details: err.to_string(),
                    }
                }
            })?;

        match timeout {
            Some(timeout_duration) => {
                Self::wait_with_timeout(&mut child, program, args, timeout_duration, start_time)
            }
            None => Self::collect_output(child, start_time),
        }
    }

    fn wait_with_timeout(
        child: &mut Child,
        program: &str,
        args: &[&str],
        timeout_duration: Duration,
        start_time: Instant,
    ) -> Result<ProcessOutput, ProcessError> {
        let poll_interval = Duration::from_millis(20);
        let deadline = Instant::now() + timeout_duration;

        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let mut stdout_buf = Vec::new();
                    let mut stderr_buf = Vec::new();

                    if let Some(stdout) = child.stdout.take() {
                        let mut reader = stdout.take(MAX_PROCESS_OUTPUT_BYTES as u64);
                        let _ = reader.read_to_end(&mut stdout_buf);
                    }
                    if let Some(stderr) = child.stderr.take() {
                        let mut reader = stderr.take(MAX_PROCESS_OUTPUT_BYTES as u64);
                        let _ = reader.read_to_end(&mut stderr_buf);
                    }

                    return Ok(ProcessOutput {
                        exit_code: status.code(),
                        success: status.success(),
                        stdout: String::from_utf8_lossy(&stdout_buf).into_owned(),
                        stderr: String::from_utf8_lossy(&stderr_buf).into_owned(),
                        duration: start_time.elapsed(),
                    });
                }
                Ok(None) => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait(); // Reap zombie process
                        return Err(ProcessError::Timeout(timeout_duration));
                    }
                    std::thread::sleep(poll_interval);
                }
                Err(e) => {
                    return Err(ProcessError::ExecutionFailed {
                        command: format!("{} {}", program, args.join(" ")),
                        details: e.to_string(),
                    });
                }
            }
        }
    }

    fn collect_output(
        mut child: Child,
        start_time: Instant,
    ) -> Result<ProcessOutput, ProcessError> {
        let mut stdout_buf = Vec::new();
        let mut stderr_buf = Vec::new();

        if let Some(stdout) = child.stdout.take() {
            let mut reader = stdout.take(MAX_PROCESS_OUTPUT_BYTES as u64);
            let _ = reader.read_to_end(&mut stdout_buf);
        }
        if let Some(stderr) = child.stderr.take() {
            let mut reader = stderr.take(MAX_PROCESS_OUTPUT_BYTES as u64);
            let _ = reader.read_to_end(&mut stderr_buf);
        }

        let status = child.wait()?;
        Ok(ProcessOutput {
            exit_code: status.code(),
            success: status.success(),
            stdout: String::from_utf8_lossy(&stdout_buf).into_owned(),
            stderr: String::from_utf8_lossy(&stderr_buf).into_owned(),
            duration: start_time.elapsed(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_executor_success() {
        let executor = ProcessExecutor::new();
        let output = executor
            .execute("echo", &["hello", "cleaner"], None)
            .unwrap();
        assert!(output.success);
        assert_eq!(output.exit_code, Some(0));
        assert!(output.stdout.contains("hello cleaner"));
    }

    #[test]
    fn test_process_executor_command_not_found() {
        let executor = ProcessExecutor::new();
        let err = executor
            .execute("non_existent_binary_xyz_123", &[], None)
            .unwrap_err();
        assert!(matches!(err, ProcessError::CommandNotFound(_)));
    }

    #[test]
    fn test_process_executor_timeout() {
        let executor = ProcessExecutor::new();
        // sleep 5 seconds with a 100ms timeout
        let err = executor
            .execute("sleep", &["5"], Some(Duration::from_millis(100)))
            .unwrap_err();
        assert!(matches!(err, ProcessError::Timeout(_)));
    }
}
