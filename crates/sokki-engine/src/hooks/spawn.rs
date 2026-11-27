//! Starting a hook's process apart from the app that runs it.
//!
//! On macOS a process an app starts counts as part of that app for privacy
//! checks: a hook started by Sokki would get Sokki's microphone,
//! Accessibility (synthetic keys and clicks) and Screen Recording grants.
//! So on macOS hooks start through `posix_spawn` with responsibility
//! disclaimed (`responsibility_spawnattrs_setdisclaim`, in libSystem since
//! macOS 10.14; terminals and developer tools start their children this
//! way). macOS then judges a hook on its own: it asks for the hook itself,
//! or refuses. If the call is ever missing, the hook does not start.
//!
//! On both platforms the hook gets its own process group (so a timeout
//! stops its children too), default signal handling, and none of the
//! app's open files except its stdin, stdout and stderr.

use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::path::Path;

/// How a hook ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exit {
    /// `None` when a signal ended it.
    pub code: Option<i32>,
}

impl Exit {
    pub fn success(self) -> bool {
        self.code == Some(0)
    }
}

pub struct Spec<'a> {
    pub program: &'a str,
    pub args: &'a [&'a str],
    /// Set on top of the app's own environment.
    pub env: &'a [(OsString, OsString)],
    pub cwd: &'a Path,
    pub stdout: &'a File,
    pub stderr: &'a File,
}

pub use imp::{spawn, Proc};

#[cfg(target_os = "macos")]
pub use imp::responsible_for;

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use std::collections::BTreeMap;
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::ptr::null_mut;

    // In libSystem since macOS 10.15; declared here because older `libc`
    // crates (the app's lock file) don't have it.
    extern "C" {
        fn posix_spawn_file_actions_addchdir_np(
            actions: *mut libc::posix_spawn_file_actions_t,
            path: *const libc::c_char,
        ) -> libc::c_int;
    }

    type SetDisclaim =
        unsafe extern "C" fn(*mut libc::posix_spawnattr_t, libc::c_int) -> libc::c_int;
    type ResponsibleFor = unsafe extern "C" fn(libc::pid_t) -> libc::pid_t;

    fn lookup(name: &std::ffi::CStr) -> *mut libc::c_void {
        unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) }
    }

    fn set_disclaim() -> Option<SetDisclaim> {
        let p = lookup(c"responsibility_spawnattrs_setdisclaim");
        (!p.is_null()).then(|| unsafe { std::mem::transmute::<*mut libc::c_void, SetDisclaim>(p) })
    }

    /// The process macOS holds responsible for `pid` in privacy checks:
    /// the app that started it, or `pid` itself when it was disclaimed.
    pub fn responsible_for(pid: u32) -> Option<u32> {
        let p = lookup(c"responsibility_get_pid_responsible_for_pid");
        if p.is_null() {
            return None;
        }
        let f = unsafe { std::mem::transmute::<*mut libc::c_void, ResponsibleFor>(p) };
        let r = unsafe { f(pid as libc::pid_t) };
        (r > 0).then_some(r as u32)
    }

    fn cstring(bytes: &[u8]) -> io::Result<CString> {
        CString::new(bytes).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a NUL byte"))
    }

    fn check(rc: libc::c_int) -> io::Result<()> {
        match rc {
            0 => Ok(()),
            e => Err(io::Error::from_raw_os_error(e)),
        }
    }

    pub struct Proc {
        pid: u32,
        done: Option<Exit>,
    }

    impl Proc {
        pub fn id(&self) -> u32 {
            self.pid
        }

        pub fn try_wait(&mut self) -> io::Result<Option<Exit>> {
            if let Some(e) = self.done {
                return Ok(Some(e));
            }
            let mut status = 0;
            let r = unsafe { libc::waitpid(self.pid as libc::pid_t, &mut status, libc::WNOHANG) };
            if r == 0 {
                return Ok(None);
            }
            if r < 0 {
                return Err(io::Error::last_os_error());
            }
            let exit = Exit {
                code: libc::WIFEXITED(status).then(|| libc::WEXITSTATUS(status)),
            };
            self.done = Some(exit);
            Ok(Some(exit))
        }

        pub fn kill(&mut self) {
            if self.done.is_none() {
                unsafe {
                    libc::kill(self.pid as libc::pid_t, libc::SIGKILL);
                }
            }
        }
    }

    /// Starts `spec` disclaimed; returns the process and its stdin.
    pub fn spawn(spec: &Spec) -> io::Result<(Proc, File)> {
        let disclaim = set_disclaim().ok_or_else(|| {
            io::Error::other(
                "macOS can't start it apart from Sokki (responsibility_spawnattrs_setdisclaim is missing), so it would get Sokki's privacy grants",
            )
        })?;
        let program = cstring(spec.program.as_bytes())?;
        let argv_c: Vec<CString> = std::iter::once(spec.program.as_bytes())
            .chain(spec.args.iter().map(|a| a.as_bytes()))
            .map(cstring)
            .collect::<io::Result<_>>()?;
        let argv: Vec<*mut libc::c_char> = argv_c
            .iter()
            .map(|c| c.as_ptr() as *mut libc::c_char)
            .chain([null_mut()])
            .collect();
        let mut env: BTreeMap<OsString, OsString> = std::env::vars_os().collect();
        for (k, v) in spec.env {
            env.insert(k.clone(), v.clone());
        }
        let env_c: Vec<CString> = env
            .iter()
            .map(|(k, v)| {
                let mut b = k.as_bytes().to_vec();
                b.push(b'=');
                b.extend_from_slice(v.as_bytes());
                cstring(&b)
            })
            .collect::<io::Result<_>>()?;
        let envp: Vec<*mut libc::c_char> = env_c
            .iter()
            .map(|c| c.as_ptr() as *mut libc::c_char)
            .chain([null_mut()])
            .collect();
        let cwd = cstring(spec.cwd.as_os_str().as_bytes())?;

        let mut fds = [0; 2];
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // Owned from here, so they close on every path; close-on-exec, so
        // no other child the app starts meanwhile inherits them.
        let (read, write) = unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) };
        for fd in fds {
            unsafe {
                libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
            }
        }

        unsafe {
            let mut fa: libc::posix_spawn_file_actions_t = null_mut();
            check(libc::posix_spawn_file_actions_init(&mut fa))?;
            let mut attr: libc::posix_spawnattr_t = null_mut();
            if let Err(e) = check(libc::posix_spawnattr_init(&mut attr)) {
                libc::posix_spawn_file_actions_destroy(&mut fa);
                return Err(e);
            }
            let spawned = (|| -> io::Result<libc::pid_t> {
                check(libc::posix_spawn_file_actions_adddup2(
                    &mut fa,
                    read.as_raw_fd(),
                    0,
                ))?;
                check(libc::posix_spawn_file_actions_adddup2(
                    &mut fa,
                    spec.stdout.as_raw_fd(),
                    1,
                ))?;
                check(libc::posix_spawn_file_actions_adddup2(
                    &mut fa,
                    spec.stderr.as_raw_fd(),
                    2,
                ))?;
                check(posix_spawn_file_actions_addchdir_np(&mut fa, cwd.as_ptr()))?;
                // Its own process group; no signal blocked; every signal
                // the app ignores back to its default (SIGPIPE above all);
                // every other open file of the app closed.
                let flags = libc::POSIX_SPAWN_SETPGROUP
                    | libc::POSIX_SPAWN_SETSIGMASK
                    | libc::POSIX_SPAWN_SETSIGDEF
                    | libc::POSIX_SPAWN_CLOEXEC_DEFAULT;
                check(libc::posix_spawnattr_setflags(
                    &mut attr,
                    flags as libc::c_short,
                ))?;
                check(libc::posix_spawnattr_setpgroup(&mut attr, 0))?;
                let mut none: libc::sigset_t = std::mem::zeroed();
                libc::sigemptyset(&mut none);
                check(libc::posix_spawnattr_setsigmask(&mut attr, &none))?;
                let mut all: libc::sigset_t = std::mem::zeroed();
                libc::sigfillset(&mut all);
                libc::sigdelset(&mut all, libc::SIGKILL);
                libc::sigdelset(&mut all, libc::SIGSTOP);
                check(libc::posix_spawnattr_setsigdefault(&mut attr, &all))?;
                check(disclaim(&mut attr, 1))?;
                let mut pid: libc::pid_t = 0;
                check(libc::posix_spawn(
                    &mut pid,
                    program.as_ptr(),
                    &fa,
                    &attr,
                    argv.as_ptr(),
                    envp.as_ptr(),
                ))?;
                Ok(pid)
            })();
            libc::posix_spawnattr_destroy(&mut attr);
            libc::posix_spawn_file_actions_destroy(&mut fa);
            let pid = spawned?;
            drop(read);
            Ok((
                Proc {
                    pid: pid as u32,
                    done: None,
                },
                write,
            ))
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::*;
    use std::process::{Child, Command, Stdio};

    pub struct Proc {
        child: Child,
    }

    impl Proc {
        pub fn id(&self) -> u32 {
            self.child.id()
        }

        pub fn try_wait(&mut self) -> io::Result<Option<Exit>> {
            Ok(self.child.try_wait()?.map(|s| Exit { code: s.code() }))
        }

        pub fn kill(&mut self) {
            let _ = self.child.kill();
        }
    }

    /// No per-app privacy grants here: the standard spawn, which closes the
    /// app's other files and resets SIGPIPE and the signal mask.
    pub fn spawn(spec: &Spec) -> io::Result<(Proc, File)> {
        let mut cmd = Command::new(spec.program);
        cmd.args(spec.args)
            .current_dir(spec.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::from(spec.stdout.try_clone()?))
            .stderr(Stdio::from(spec.stderr.try_clone()?));
        for (k, v) in spec.env {
            cmd.env(k, v);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let mut child = cmd.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("no stdin"))?;
        #[cfg(unix)]
        let stdin = {
            use std::os::fd::{FromRawFd, IntoRawFd};
            unsafe { File::from_raw_fd(stdin.into_raw_fd()) }
        };
        Ok((Proc { child }, stdin))
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    /// The hook is responsible for itself, not the process that started
    /// it (here the test, standing in for Sokki).
    #[test]
    fn macos_holds_the_hook_not_the_app_responsible() {
        let dir = std::env::temp_dir().join(format!("sokki-spawn-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = File::create(dir.join("log")).unwrap();
        let (mut p, stdin) = spawn(&Spec {
            program: "/bin/sh",
            args: &["-c", "sleep 1"],
            env: &[],
            cwd: &dir,
            stdout: &log,
            stderr: &log,
        })
        .unwrap();
        drop(stdin);
        std::thread::sleep(std::time::Duration::from_millis(200));
        let me = std::process::id();
        assert_eq!(
            responsible_for(p.id()),
            Some(p.id()),
            "the hook answers for itself"
        );
        assert_ne!(responsible_for(p.id()), responsible_for(me));
        while p.try_wait().unwrap().is_none() {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
