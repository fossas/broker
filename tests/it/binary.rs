//! Tests related to the binary itself.
//!
//! TODO: test these in Windows as well
//!       https://www.reddit.com/r/rust/comments/u7q1yx/comment/i5jkiqh/?utm_source=share&utm_medium=web2x&context=3

use std::path::Path;

use tempfile::TempDir;

use crate::helper::temp_config;

macro_rules! run {
    (broker => $($arg:tt)*) => {{
        let bin = env!("CARGO_BIN_EXE_broker");
        run!(bin => $($arg)*)
    }};
    ($name:expr => $($arg:tt)*) => {{
        use std::os::unix::process::CommandExt;
        let command = format!($($arg)*);
        let root = env!("CARGO_MANIFEST_DIR");

        let args = command.split_ascii_whitespace().collect::<Vec<_>>();
        std::process::Command::new($name)
            .args(&args)
            .current_dir(root)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .process_group(0)
            .spawn()
            .expect("must start process")
    }};
}

#[track_caller]
#[cfg(target_family = "unix")]
fn interrupt(pid: u32) {
    let output = run!("kill" => "-INT {pid}")
        .wait_with_output()
        .expect("must run 'kill'");

    // Just warn so that error messages from broker show up in test failure.
    if !output.status.success() {
        eprintln!("[warn] failed to interrupt {pid}: {output:?}")
    }
}

#[track_caller]
#[cfg(target_family = "unix")]
fn run_and_interrupt_broker(tmp: &TempDir, config_path: &Path) {
    let config_path = config_path.to_string_lossy().to_string();
    let data_root = tmp.path().to_string_lossy().to_string();
    let child = run!(broker => "run -c {config_path} -r {data_root}");
    let pid = child.id();
    println!("pid: {pid}");

    // Run Broker.
    let child = std::thread::spawn(move || child.wait_with_output());

    // Wait a moment for Broker to start, then interrupt it.
    //
    // Ideally we'd instead wait for Broker to output some text in its stdout.
    // This would make things a lot more complicated for seemingly little benefit,
    // so leave this here unless it becomes flaky or tests start taking way too long.
    std::thread::sleep(std::time::Duration::from_millis(1000));
    interrupt(pid);

    // Test the output.
    let output = child
        .join()
        .expect("join child wait thread")
        .expect("child wasn't running");

    let stdout = String::from_utf8(output.stdout).expect("stdout must have been utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr must have been utf8");
    assert!(
        stderr.contains("Shut down at due to OS signal"),
        "ensure interrupted\n- status: {:?}\n- stdout: {stdout}\n- stderr: {stderr}",
        output.status,
    );
}

#[test]
#[cfg(target_family = "unix")]
fn interrupts() {
    let (tmp, config_path) = temp_config!();
    run_and_interrupt_broker(&tmp, &config_path);
}

#[test]
#[cfg(target_family = "unix")]
fn cleans_up_queue_lock_file() {
    let (tmp, config_path) = temp_config!();

    // Literally just run it twice.
    // Ensures that the second time boots instead of failing due to a queue lock file issue.
    run_and_interrupt_broker(&tmp, &config_path);

    // Wait for the sqlite db to unlock.
    //
    // Ideally we'd instead wait for the actual unlock.
    // This would make things a lot more complicated for seemingly little benefit,
    // so leave this here unless it becomes flaky or tests start taking way too long.
    std::thread::sleep(std::time::Duration::from_millis(1000));
    run_and_interrupt_broker(&tmp, &config_path);
}
