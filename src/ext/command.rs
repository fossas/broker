//! Extensions to commands.

use std::{
    fmt::Display,
    process::{Command, Output},
};

use getset::Getters;

/// The description of a command.
#[derive(Debug, Clone, Getters)]
#[getset(get = "pub")]
pub struct CommandDescription {
    /// The name of the command.
    name: String,

    /// The arguments to the command.
    args: Vec<String>,

    /// The environment variables provided to the command.
    envs: Vec<String>,

    /// The status code of the command, if any.
    status: Option<i32>,

    /// The stdout of the command, if any.
    stdout: Option<String>,

    /// The stderr of the command, if any.
    stderr: Option<String>,
}

impl Display for CommandDescription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", self.name())?;
        writeln!(f, "args: {}", self.display_args())?;
        writeln!(f, "env: {}", self.display_envs())?;
        if let Some(status) = &self.status {
            writeln!(f, "status: {}", status)?;
        }
        if let Some(stdout) = &self.stdout {
            writeln!(f, "stdout: '{}'", stdout.trim())?;
        }
        if let Some(stderr) = &self.stderr {
            writeln!(f, "stderr: '{}'", stderr.trim())?;
        }
        Ok(())
    }
}

impl CommandDescription {
    /// Enrich the command description with the command's stdout.
    pub fn with_stdout(self, stdout: String) -> Self {
        Self {
            stdout: Some(stdout),
            ..self
        }
    }

    /// Enrich the command description with the command's stderr.
    pub fn with_stderr(self, stderr: String) -> Self {
        Self {
            stderr: Some(stderr),
            ..self
        }
    }

    /// Enrich the command description with the command's status code.
    pub fn with_status(self, status: i32) -> Self {
        Self {
            status: Some(status),
            ..self
        }
    }

    /// Enrich the command description with the command output.
    /// Convenience method for `with_stdout`, `with_stderr`, and `with_status` when the user has an `Output`.
    pub fn with_output(self, output: &Output) -> Self {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        Self {
            stdout: Some(stdout.to_string()),
            stderr: Some(stderr.to_string()),
            status: Some(output.status.code().unwrap_or(-1)),
            ..self
        }
    }

    /// Joins the args into a single string for display.
    ///
    /// This is preferred over using the debug implementation because if the args contain a path,
    /// and Broker is running on Windows, the debug implementation doubles backslashes.
    pub fn display_args(&self) -> String {
        let joined = self
            .args
            .iter()
            .map(|a| format!(r#""{a}""#))
            .collect::<Vec<_>>()
            .join(", ");
        format!("[{}]", joined)
    }

    /// Joins the envs into a single string for display.
    ///
    /// This is preferred over using the debug implementation because if the args contain a path,
    /// and Broker is running on Windows, the debug implementation doubles backslashes.
    pub fn display_envs(&self) -> String {
        let joined = self
            .envs
            .iter()
            .map(|a| format!(r#""{a}""#))
            .collect::<Vec<_>>()
            .join(", ");
        format!("[{}]", joined)
    }
}

/// Supports rendering a command in the Broker standardized form.
pub trait DescribeCommand {
    /// Provide a description of a command in the Broker standardized form.
    ///
    /// Most users will want to just use the `Display` implementation of `CommandDescription` directly,
    /// but if desired this can be used to get the component parts of a command description without rendering it.
    ///
    /// Note that when the command relies on paths that are not valid UTF-8,
    /// these are converted to string lossily.
    fn describe(&self) -> CommandDescription;
}

impl DescribeCommand for Command {
    fn describe(&self) -> CommandDescription {
        let name = self.get_program().to_string_lossy().to_string();
        let args = self
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        let envs = self
            .get_envs()
            .map(|(key, value)| {
                let key = key.to_string_lossy();
                if let Some(value) = value {
                    let value = value.to_string_lossy();
                    format!("{}={}", key, value)
                } else {
                    format!("{}=<REMOVED>", key)
                }
            })
            .collect::<Vec<_>>();

        CommandDescription {
            name,
            args,
            envs,
            stdout: None,
            stderr: None,
            status: None,
        }
    }
}

impl DescribeCommand for tokio::process::Command {
    fn describe(&self) -> CommandDescription {
        self.as_std().describe()
    }
}
