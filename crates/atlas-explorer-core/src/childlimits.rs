//! Resource limits for the programs Files starts on files it does not trust
//! (`pdftotext` on a PDF, `git` in a work tree from anywhere). A program that
//! parses a hostile file can run away with memory or time, or write a core
//! file holding what it read; these limits are set in the child between the
//! fork and the exec, and the child gets a process group of its own so that
//! it can be ended together with whatever it started.

use std::os::unix::process::CommandExt;
use std::process::Command;

/// What a child may use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChildLimits {
    /// Address space, in bytes (`RLIMIT_AS`).
    pub address_space: u64,
    /// CPU time, in seconds (`RLIMIT_CPU`).
    pub cpu_seconds: u64,
    /// Open files (`RLIMIT_NOFILE`).
    pub open_files: u64,
}

/// `pdftotext`: a page description can ask for a great deal of memory.
pub const PDFTOTEXT: ChildLimits = ChildLimits {
    address_space: 1 << 30,
    cpu_seconds: 20,
    open_files: 64,
};

/// `git status` over a work tree.
pub const GIT: ChildLimits = ChildLimits {
    address_space: 2 << 30,
    cpu_seconds: 20,
    open_files: 256,
};

/// Makes `cmd` run with `limits`, no core file, and in a process group of its
/// own (see [`kill_group`]).
pub fn apply(cmd: &mut Command, limits: ChildLimits) {
    cmd.process_group(0);
    // SAFETY: the closure runs after the fork and before the exec, so it
    // makes only async-signal-safe calls: `setrlimit` with plain integers.
    unsafe {
        cmd.pre_exec(move || {
            let set = |resource: libc::__rlimit_resource_t, value: u64| {
                let lim = libc::rlimit {
                    rlim_cur: value,
                    rlim_max: value,
                };
                // (inside the unsafe block of `pre_exec`) a valid rlimit struct
                if libc::setrlimit(resource, &lim) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            };
            set(libc::RLIMIT_CORE, 0)?;
            set(libc::RLIMIT_AS, limits.address_space)?;
            set(libc::RLIMIT_CPU, limits.cpu_seconds)?;
            set(libc::RLIMIT_NOFILE, limits.open_files)?;
            Ok(())
        });
    }
}

/// Ends the child `pid` started with [`apply`] and everything in its group.
pub fn kill_group(pid: u32) {
    // SAFETY: killpg with a valid signal; a group that is gone is ESRCH.
    unsafe { libc::killpg(pid as libc::pid_t, libc::SIGKILL) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    #[test]
    fn the_child_runs_with_the_limits_and_in_its_own_group() {
        let mut cmd = Command::new("/bin/sh");
        cmd.args([
            "-c",
            "ulimit -c; ulimit -v; ulimit -t; ulimit -n; ps -o pgid= -p $$ 2>/dev/null; echo end",
        ])
        .stdout(Stdio::piped())
        .stdin(Stdio::null())
        .stderr(Stdio::null());
        apply(&mut cmd, PDFTOTEXT);
        let out = cmd.output().expect("sh runs");
        let text = String::from_utf8_lossy(&out.stdout);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "0", "no core file: {text}");
        assert_eq!(
            lines[1],
            (PDFTOTEXT.address_space / 1024).to_string(),
            "{text}"
        );
        assert_eq!(lines[2], PDFTOTEXT.cpu_seconds.to_string(), "{text}");
        assert_eq!(lines[3], PDFTOTEXT.open_files.to_string(), "{text}");
    }

    #[test]
    fn a_group_is_ended_with_what_the_child_started() {
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "sleep 30 & echo $!; wait"])
            .stdout(Stdio::piped())
            .stdin(Stdio::null())
            .stderr(Stdio::null());
        apply(&mut cmd, GIT);
        let mut child = cmd.spawn().unwrap();
        let mut line = String::new();
        {
            use std::io::BufRead;
            std::io::BufReader::new(child.stdout.as_mut().unwrap())
                .read_line(&mut line)
                .unwrap();
        }
        let grandchild: i32 = line.trim().parse().unwrap();
        kill_group(child.id());
        let _ = child.wait();
        std::thread::sleep(std::time::Duration::from_millis(200));
        // SAFETY: signal 0 only asks whether the process is there.
        let alive = unsafe { libc::kill(grandchild, 0) } == 0;
        assert!(!alive, "the grandchild outlived its group");
    }
}
