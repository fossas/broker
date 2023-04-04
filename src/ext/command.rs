//! Extensions to commands.

use std::{
    ffi::{OsStr, OsString},
    fmt::Display,
    ops::Deref,
    path::PathBuf,
    process::{ExitStatus, Stdio},
};

use aho_corasick::AhoCorasick;
use getset::Getters;
use itertools::Itertools;
use thiserror::Error;
use tokio::process::{Child, ChildStderr, ChildStdout};

use crate::ext::secrecy::REDACTION_LITERAL;

use super::{result::WrapOk, secrecy::ComparableSecretString};

/// Any error encountered running the program.
#[derive(Debug, Error)]
pub enum Error {
    /// An underlying IO error occurred.
    #[error("underlying IO error: {}", .0.trim())]
    IO(String),
}

impl Error {
    /// Create an `IO` variant from the provided IO error.
    /// Redacts any instances of secrets in the error message.
    fn io(err: std::io::Error, engine: &AhoCorasick) -> Self {
        let err_message = format!("{err:#}");
        let redacted = redact_str(&err_message, engine);
        Self::IO(redacted)
    }
}

/// Broker makes extensive use of commands which may be passed arguments or environment values
/// that are secret, and should be stripped from debugging output.
///
/// However, when running standard commands (which take strings),
/// and then displaying the command metadata on error (which just prints those strings),
/// it's not really possible to know whether something should be redacted.
///
/// This type is a replacement for `Command`, which:
/// - Knows the kinds of values being passed in, since it accepts strings _or_ secrets.
/// - Knows how to safely display those values.
#[derive(Debug, Clone)]
pub struct Command {
    /// Arguments to the command.
    args: Vec<Value>,

    /// The list of environment vars to modify.
    /// `Some(value)` means they're set to `value`.
    /// `None` means they're cleared from the environment.
    envs: Vec<(String, Option<Value>)>,

    /// The working directory for the command.
    /// If not specified, defaults to the working directory of the current process.
    working_dir: Option<PathBuf>,

    /// Commands really reference paths on the local file system,
    /// which may or may not be UTF8.
    name: OsString,
}

impl Command {
    /// Create a new command, which will eventually execute the provided binary.
    pub fn new<S: AsRef<OsStr>>(command: S) -> Self {
        Self {
            args: Vec::new(),
            envs: Vec::new(),
            name: command.as_ref().to_owned(),
            working_dir: None,
        }
    }

    /// Adds an argument to pass to the program.
    pub fn arg<V: Into<Value>>(mut self, value: V) -> Self {
        self.args.push(value.into());
        self
    }

    /// Adds an argument to pass to the program,
    /// which is converted to a secret if needed.
    pub fn arg_secret<S: Into<ComparableSecretString>>(mut self, secret: S) -> Self {
        self.args.push(Value::new_secret(secret));
        self
    }

    /// Adds an argument to pass to the program as plain text.
    pub fn arg_plain<S: Into<String>>(mut self, arg: S) -> Self {
        self.args.push(Value::new_plain(arg));
        self
    }

    /// Adds multiple arguments to pass to the program.
    pub fn args<V, I>(mut self, values: I) -> Self
    where
        V: Into<Value>,
        I: IntoIterator<Item = V>,
    {
        let values = values.into_iter().map(|v| v.into()).collect_vec();
        self.args.extend_from_slice(&values);
        self
    }

    /// Set an environment variable for the child.
    pub fn env<K: Into<String>, V: Into<Value>>(mut self, key: K, value: V) -> Self {
        self.envs.push((key.into(), Some(value.into())));
        self
    }

    /// Set an environment variable for the child as plain text.
    pub fn env_plain<K: Into<String>, V: Into<String>>(mut self, key: K, value: V) -> Self {
        let value = Value::new_plain(value.into());
        self.envs.push((key.into(), Some(value)));
        self
    }

    /// Set an environment variable for the child as a secret.
    pub fn env_secret<K, V>(mut self, key: K, value: V) -> Self
    where
        K: Into<String>,
        V: Into<ComparableSecretString>,
    {
        let value = Value::new_secret(value.into());
        self.envs.push((key.into(), Some(value)));
        self
    }

    /// Set multiple environment variables for the child.
    pub fn envs<K, V, I>(mut self, pairs: I) -> Self
    where
        K: Into<String>,
        V: Into<Value>,
        I: IntoIterator<Item = (K, V)>,
    {
        let pairs = pairs.into_iter().map(|(k, v)| (k.into(), Some(v.into())));
        self.envs.extend(pairs);
        self
    }

    /// Clear an environment variable.
    pub fn env_remove<K: Into<String>>(mut self, key: K) -> Self {
        // Note: unimplemented for now because no need yet,
        // but if we need to clear everything just implement `cmd.clear_env()`
        // in `as_cmd`.
        self.envs.push((key.into(), None));
        self
    }

    /// Customizes the working directory for the command.
    pub fn current_dir<P: Into<PathBuf>>(mut self, dir: P) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    /// Executes the command as a child process,
    /// waiting for it to finish and collecting all of its output.
    pub async fn output(&self) -> Result<Output, Error> {
        let mut cmd = self.as_cmd();
        let redact = self.redaction_engine();

        let output = cmd
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|err| Error::io(err, &redact))?;

        Output::new(output, redact, self.describe()).wrap_ok()
    }

    /// Spawns the command as a child process, returning a handle to it
    /// that can be used to read the output in a streaming fashion.
    pub fn stream(&self) -> Result<OutputStream, Error> {
        let mut cmd = self.as_cmd();
        let engine = self.redaction_engine();

        let child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|err| Error::io(err, &engine))?;

        Ok(OutputStream {
            child,
            engine,
            description: self.describe(),
        })
    }

    /// Create an underlying Tokio command to run this binary.
    ///
    /// Note that secrets are exposed as part of this; it's important to not
    /// log any of its output directly.
    fn as_cmd(&self) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(&self.name);

        if let Some(working_dir) = &self.working_dir {
            cmd.current_dir(working_dir);
        }

        for arg in &self.args {
            cmd.arg(arg.expose_secret());
        }

        for (key, value) in &self.envs {
            match value {
                Some(value) => {
                    cmd.env(key, value.expose_secret());
                }
                None => {
                    cmd.env_remove(key);
                }
            }
        }

        cmd
    }

    /// Generate a redaction engine to be used to redact output.
    fn redaction_engine(&self) -> AhoCorasick {
        let values = self
            .envs
            .iter()
            .filter_map(|(_, v)| v.as_ref())
            .chain(self.args.iter())
            .cloned();
        redaction_engine(values)
    }
}

/// The output of a command which buffered its stdout and stderr streams.
///
/// # Description
///
/// This struct implements [`CommandDescriber`],
/// where the description is for the original command that
/// resulted in this output.
#[derive(Clone)]
pub struct Output {
    description: Description,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    status: ExitStatus,
    redact: AhoCorasick,
}

impl Output {
    fn new(value: std::process::Output, redact: AhoCorasick, desc: Description) -> Self {
        Self {
            stdout: value.stdout,
            stderr: value.stderr,
            status: value.status,
            description: desc,
            redact,
        }
    }
}

impl CommandDescriber for Output {
    fn describe(&self) -> Description {
        self.description
            .clone()
            .with_status(self.exit_code())
            .with_stderr(self.stderr_string_lossy())
            .with_stdout(self.stdout_string_lossy())
    }
}

impl OutputProvider for Output {
    fn stdout(&self) -> Vec<u8> {
        redact_bytes(&self.stdout, &self.redact)
    }

    fn stderr(&self) -> Vec<u8> {
        redact_bytes(&self.stderr, &self.redact)
    }

    fn status(&self) -> ExitStatus {
        self.status
    }
}

/// The output of a streaming command.
///
/// Use `take_stderr` and/or `take_stdout` to take ownership of the corresponding output handle.
/// Use `wait` to wait for the process to close.
///
/// # Secrets
///  
/// The caller is responsible for ensuring that the output is redacted,
/// if this output is to be printed.
/// Use the [`redact_str`] or [`redact_bytes`] methods on this struct
/// to perform redaction.
///
/// # Description
///
/// This struct implements [`CommandDescriber`],
/// where the description is for the original command that
/// resulted in this stream.
pub struct OutputStream {
    child: Child,
    engine: AhoCorasick,
    description: Description,
}

impl OutputStream {
    /// Take stderr from the output.
    ///
    /// The caller is responsible for ensuring that the output is redacted,
    /// if this output is to be printed.
    /// Use the [`redact_str`] or [`redact_bytes`] methods on this struct
    /// to perform redaction.
    ///
    /// As of running this function, the caller now owns stderr;
    /// successive calls panic.
    pub fn take_stderr(&mut self) -> ChildStderr {
        match self.child.stderr.take() {
            Some(stderr) => stderr,
            None => panic!("stderr must be piped"),
        }
    }

    /// Take stdout from the output.
    ///
    /// The caller is responsible for ensuring that the output is redacted,
    /// if this output is to be printed. Use [`redacter`] for this purpose.
    ///
    /// As of running this function, the caller now owns stdout;
    /// successive calls panic.
    pub fn take_stdout(&mut self) -> ChildStdout {
        match self.child.stdout.take() {
            Some(stdout) => stdout,
            None => panic!("stdout must be piped"),
        }
    }

    /// Wait for the child process to exit.
    pub async fn wait(&mut self) -> Result<ExitStatus, Error> {
        self.child
            .wait()
            .await
            .map_err(|err| Error::io(err, &self.engine))
    }

    /// Create a redactor capable of redacting outputs for this command.
    pub fn redacter(&self) -> Redacter {
        Redacter::new(self.engine.clone())
    }
}

impl CommandDescriber for OutputStream {
    fn describe(&self) -> Description {
        self.description.clone()
    }
}

/// Handles the possibility of being either a secret or a string.
///
/// It's used for any value provided to a command:
/// - Env variable values (not the variable name)
/// - Arguments
///
/// When creating a new `CommandValue`, use the appropriate `new` function:
/// - `new_secret`: This value is used as a secret and will be redacted from any debugging output.
/// - `new_plain`: This value is used as plain text, and is not redacted.
///
/// `CommandValue` doesn't implement `From` for its input types, because it's possible
/// that the user wants to create a `Secret` variant from a `String`,
/// and a `From` implementation cannot possibly divine that intent.
///
/// Broker automatically redacts values debugged _by Broker_, but cannot control the stdout
/// or stderr of child processes. For these, the specialized output provided
/// by the [`Command`] struct in this module redacts secrets by performing replacements
/// on secret values in the output.
#[derive(Debug, Clone)]
pub enum Value {
    /// Secrets are redacted from debugging output.
    Secret(ComparableSecretString),

    /// Secrets with a specific plain string set to use for `Display` and `Debug` implementations.
    ///
    /// Most secrets are simply redacted in place, but sometimes it's useful to display a different
    /// value entirely; this variant makes that possible.
    ///
    /// See [`Value::format_secret`] for a convenient way to construct this variant
    /// using syntax inspired by the `format!()` macro.
    SecretDisplay(String, ComparableSecretString),

    /// Plain text, not redacted in debugging output.
    Plain(String),
}

impl Value {
    /// Create a new instance as a `Secret` variant.
    ///
    /// `Secret` variants are automatically redacted in debugging output.
    pub fn new_secret<S: Into<ComparableSecretString>>(value: S) -> Self {
        Self::Secret(value.into())
    }

    /// Create a new `Secret` variant with the provided format string.
    ///
    /// This format string is slightly different than the ones used in the `format!` (and similar) macros,
    /// since this is done at runtime instead of compile time.
    /// Users **must** use the literal `{secret}` in the format string
    /// to interpolate the exposed version of the provided secret.
    ///
    /// The formatter looks for instances of the literal `{secret}` in the format string, and:
    /// - For the generated secret value, interpolates the exposed secret into the string, storing the entire thing as a secret.
    /// - For the generated plain description, interpolates the secret redaction literal into the string.
    ///
    /// For example, if secrets are redacted with the value `<REDACTED>`, then the input:
    /// ```ignore
    /// Value::new_secret_format(
    ///     "AUTHORIZATION: Basic {secret}",
    ///     "username:password",
    /// )
    /// ```
    ///
    /// Has a `Debug` and `Display` implementation that shows the following:
    /// ```not_rust
    /// AUTHORIZATION: Basic <REDACTED>
    /// ```
    ///
    /// Meanwhile, the actual secret value generated and stored in memory is the following:
    /// ```not_rust
    /// AUTHORIZATION: Basic username:password
    /// ```
    ///
    /// The literal actually used at runtime is managed by [`REDACTION_LITERAL`].
    /// If there are _multiple_ `{secret}` literals, they are all replaced.
    /// If there are _no_ `{secret}` literals, this function panicks, as this is almost certainly
    /// a programmer error. If this is actually desired, either:
    /// - Create a `Value::SecretDisplay` instance yourself.
    /// - Use `Value::Plain`.
    ///
    /// This interpolation is done when the variant is constructed;
    /// the values stored in this enum variant are not interpolated further.
    ///
    /// # Background
    ///
    /// Generally intended to be used when an entire string is provided as a value, but only part of it is actually secret,
    /// to support showing as much as possible of the input.
    ///
    /// For example, the git arguments:
    /// ```not_rust
    /// git -c 'http.extraHeader=AUTHORIZATION: Basic username:password'
    /// ```
    ///
    /// Would normally be have to be passed in as:
    /// ```ignore
    /// vec![Value::new_plain("-c"), Value::new_secret("http.extraHeader=AUTHORIZATION: Basic username:password")]
    /// ```
    ///
    /// Which would result in less than helpful output, since the entire secret is redacted:
    /// ```not_rust
    /// run git: /some/path/to/git
    /// args: ["-c", "<REDACTED>"]
    /// ```
    ///
    /// But using this function:
    /// ```ignore
    /// vec![
    ///     Value::new_plain("-c"),
    ///     Value::new_secret_format(
    ///         "http.extraHeader=AUTHORIZATION: Basic {secret}"
    ///         "username:password",
    ///     ),
    /// ]
    /// ```
    ///
    /// We can get back more useful debugging output while still protecting the secret:
    /// ```ignore
    /// run git: /some/path/to/git
    /// args: ["-c", "http.extraHeader=AUTHORIZATION: Basic <REDACTED>"]
    /// ```
    ///
    /// Note that this only applies to debug output generated by Broker;
    /// redactions applied to output generated by the child process
    /// uses the simpler initial form.
    pub fn format_secret<F, S>(format: F, value: S) -> Self
    where
        F: Into<String>,
        S: Into<ComparableSecretString>,
    {
        const SECRET_LITERAL: &str = "{secret}";

        let format = format.into();
        let value = value.into();
        Self::SecretDisplay(
            format.replace(SECRET_LITERAL, REDACTION_LITERAL),
            ComparableSecretString::from(format.replace(SECRET_LITERAL, value.expose_secret())),
        )
    }

    /// Create a new instance as a `Plain` variant.
    ///
    /// `Plain` variants are not redacted in debugging output.
    pub fn new_plain<S: Into<String>>(value: S) -> Self {
        Self::Plain(value.into())
    }

    /// Used to view the inner value, potentially exposing the secret (if this is a secret value).
    fn expose_secret(&self) -> &str {
        match self {
            Value::Secret(secret) => secret.expose_secret(),
            Value::SecretDisplay(_, secret) => secret.expose_secret(),
            Value::Plain(value) => value,
        }
    }
}

impl Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Secret(_) => write!(f, "{REDACTION_LITERAL}"),
            Value::SecretDisplay(plain, _) => write!(f, "{plain}"),
            Value::Plain(value) => write!(f, "{value}"),
        }
    }
}

/// The description of a command.
#[derive(Debug, Clone, Getters, Default)]
#[getset(get = "pub")]
pub struct Description {
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

impl Display for Description {
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

impl Description {
    /// Create a new description.
    fn new(name: String, args: Vec<String>, envs: Vec<String>) -> Self {
        Self {
            name,
            args,
            envs,
            ..Default::default()
        }
    }

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
    pub fn with_output<O: OutputProvider>(self, output: O) -> Self {
        Self {
            stdout: Some(output.stdout_string_lossy()),
            stderr: Some(output.stderr_string_lossy()),
            status: Some(output.exit_code()),
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
pub trait CommandDescriber {
    /// Provide a description of a command in the Broker standardized form.
    ///
    /// Most users will want to just use the `Display` implementation of `CommandDescription` directly,
    /// but if desired this can be used to get the component parts of a command description without rendering it.
    ///
    /// Note that when the command relies on paths that are not valid UTF-8,
    /// these are converted to string lossily.
    fn describe(&self) -> Description;
}

impl CommandDescriber for Command {
    fn describe(&self) -> Description {
        let name = self.name.to_string_lossy().to_string();
        let args = self.args.iter().map(|arg| arg.to_string()).collect_vec();
        let envs = self
            .envs
            .iter()
            .map(|(key, value)| match value {
                Some(value) => format!("{key}={value}"),
                None => format!("{key}=<REMOVED>"),
            })
            .collect_vec();

        Description::new(name, args, envs)
    }
}

impl CommandDescriber for std::process::Command {
    fn describe(&self) -> Description {
        let name = self.get_program().to_string_lossy().to_string();
        let args = self
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        let envs = self
            .get_envs()
            .map(|(key, value)| (key.to_string_lossy(), value.map(OsStr::to_string_lossy)))
            .map(|(key, value)| match value {
                Some(value) => format!("{key}={value}"),
                None => format!("{key}=<REMOVED>"),
            })
            .collect_vec();

        Description::new(name, args, envs)
    }
}

impl CommandDescriber for tokio::process::Command {
    fn describe(&self) -> Description {
        self.as_std().describe()
    }
}

impl<T> CommandDescriber for &T
where
    T: CommandDescriber,
{
    fn describe(&self) -> Description {
        self.deref().describe()
    }
}

impl CommandDescriber for Description {
    fn describe(&self) -> Description {
        self.clone()
    }
}

/// Describes functionality for reading the output of commands.
pub trait OutputProvider {
    /// The stdout of the command.
    fn stdout(&self) -> Vec<u8>;

    /// The stderr of the command.
    fn stderr(&self) -> Vec<u8>;

    /// The exit status of the command.
    fn status(&self) -> ExitStatus;

    /// The stdout of the command, lossily converted to a string.
    /// During this conversion, any invalid UTF-8 sequences are replaced with
    /// `U+FFFD`, which looks like this: �
    ///
    /// If it matters to the user whether the output is valid UTF8
    /// (as opposed to lossily converting), prefer getting bytes with
    /// the `stdout` method and parsing manually.
    fn stdout_string_lossy(&self) -> String {
        String::from_utf8_lossy(&self.stdout()).to_string()
    }

    /// The stderr of the command, lossily converted to a string.
    /// During this conversion, any invalid UTF-8 sequences are replaced with
    /// `U+FFFD`, which looks like this: �
    ///
    /// If it matters to the user whether the output is valid UTF8
    /// (as opposed to lossily converting), prefer getting bytes with
    /// the `stderr` method and parsing manually.
    fn stderr_string_lossy(&self) -> String {
        String::from_utf8_lossy(&self.stderr()).to_string()
    }

    /// The exit code of the command.
    /// Defaults to `-1` if not available.
    ///
    /// If it matters to the user whether the exit code was actually available,
    /// use the `status` method and access the exit code manually.
    fn exit_code(&self) -> i32 {
        self.status().code().unwrap_or(-1)
    }
}

impl OutputProvider for std::process::Output {
    fn stdout(&self) -> Vec<u8> {
        self.stdout.clone()
    }

    fn stderr(&self) -> Vec<u8> {
        self.stderr.clone()
    }

    fn status(&self) -> ExitStatus {
        self.status
    }
}

impl<T> OutputProvider for &T
where
    T: OutputProvider,
{
    fn stdout(&self) -> Vec<u8> {
        self.deref().stdout()
    }

    fn stderr(&self) -> Vec<u8> {
        self.deref().stderr()
    }

    fn status(&self) -> ExitStatus {
        self.deref().status()
    }
}

/// Manages redacting outputs.
#[derive(Debug, Clone)]
pub struct Redacter {
    engine: AhoCorasick,
}

impl Redacter {
    /// Create a new redaction engine.
    fn new(engine: AhoCorasick) -> Self {
        Self { engine }
    }

    /// Redacts secrets provided to the original command from the provided string.
    pub fn redact_str(&self, input: &str) -> String {
        redact_str(input, &self.engine)
    }

    /// Redacts secrets provided to the original command from the provided bytes.
    pub fn redact_bytes(&self, input: &[u8]) -> Vec<u8> {
        redact_bytes(input, &self.engine)
    }
}

/// Generically redacts the provided bytes with any match found by the provided engine.
fn redact_str(provided: &str, engine: &AhoCorasick) -> String {
    let mut redacted = String::new();
    engine.replace_all_with(provided, &mut redacted, |_, _, dst| {
        dst.push_str(REDACTION_LITERAL);
        true
    });
    redacted
}

/// Generically redacts the provided bytes with any match found by the provided engine.
fn redact_bytes(provided: &[u8], engine: &AhoCorasick) -> Vec<u8> {
    let mut redacted = Vec::new();
    engine.replace_all_with_bytes(provided, &mut redacted, |_, _, dst| {
        dst.extend_from_slice(REDACTION_LITERAL.as_bytes());
        true
    });
    redacted
}

/// Generate an Aho-Corasick engine.
fn redaction_engine<I: IntoIterator<Item = Value>>(values: I) -> AhoCorasick {
    let patterns = values
        .into_iter()
        .filter(|v| matches!(v, Value::Secret(_)))
        .map(|v| v.expose_secret().to_string())
        .collect_vec();
    AhoCorasick::new_auto_configured(&patterns)
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn redacts_generic() {
        let provided = "Aute aliqua ad commodo in ullamco aliqua.";
        let expected = "Aute <REDACTED> ad commodo in <REDACTED> <REDACTED>.";

        let engine = redaction_engine([
            Value::new_secret("aliqua"),
            Value::new_plain("commodo"),
            Value::new_secret("ullamco"),
        ]);

        let redacted = redact_str(provided, &engine);
        assert_eq!(redacted, expected);
    }

    #[test]
    fn redacts_generic_bytes() {
        let provided = b"Aute aliqua ad commodo in ullamco aliqua.";
        let expected = b"Aute <REDACTED> ad commodo in <REDACTED> <REDACTED>.";

        let engine = redaction_engine([
            Value::new_secret("aliqua"),
            Value::new_plain("commodo"),
            Value::new_secret("ullamco"),
        ]);

        let redacted = redact_bytes(provided, &engine);
        assert_eq!(redacted, expected);
    }
}
