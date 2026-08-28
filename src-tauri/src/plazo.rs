//! Plazo of gestor commands (#11): a hung gestor becomes a visible
//! error, never a frozen app.
//!
//! Process supervision lives apart from the kernel's parsing/execution
//! concerns: the deadline type, the per-command-class constants, the
//! user-visible timeout error and the escalated termination. The engine
//! that arms the watchdog is [`crate::kernel::correr_streaming`].

use std::sync::Mutex;
use std::time::Duration;

/// Deadline of a gestor command: `total` until the escalation starts,
/// `grace` between the courteous signal and the forced kill (#11). A hung
/// gestor becomes an error, never a frozen app.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Plazo {
    pub(crate) total: Duration,
    pub(crate) grace: Duration,
}

/// Queries (`ls`, `outdated`, `--version`): seconds against the registry.
pub(crate) const PLAZO_CONSULTA: Plazo = Plazo {
    total: Duration::from_secs(60),
    grace: Duration::from_secs(5),
};

/// Installations (`install`/`add -g`): they legitimately take minutes.
pub(crate) const PLAZO_INSTALACION: Plazo = Plazo {
    total: Duration::from_secs(300),
    grace: Duration::from_secs(5),
};

/// The expired-deadline error the UI will show, with the binary that
/// never answered.
pub(crate) fn plazo_vencido(cmd: &std::process::Command, total: Duration) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        format!(
            "{} no respondió en {} s (proceso finalizado)",
            cmd.get_program().to_string_lossy(),
            total.as_secs()
        ),
    )
}

/// Escalated termination of a hung child: courteous signal, `grace`, then
/// forced kill — and the reaping that leaves no zombie. Returns whether
/// it had to escalate at all (the child was still alive when the deadline
/// expired): a child that died on its own right at the boundary is NOT a
/// timeout.
///
/// Gestors arrive through shims (`sh`/`npm.cmd` → node) and the direct
/// child is just the wrapper: a kill to it leaves the grandchildren alive
/// holding the output pipe. On unix the child runs in its OWN process
/// group (see the spawn) and the escalation signals the whole group; on
/// Windows `taskkill /T /F` ends the tree.
pub(crate) fn finalizar(hijo: &Mutex<std::process::Child>, grace: Duration) -> bool {
    let Ok(mut hijo) = hijo.lock() else {
        return false;
    };
    if hijo.try_wait().map(|t| t.is_some()).unwrap_or(false) {
        return false; // it died on its own while we armed the watch
    }
    #[cfg(unix)]
    {
        let grupo = -(hijo.id() as i32);
        // SIGTERM to the group lets the gestor close its installation
        // orderly.
        let ya_no_esta = unsafe { libc::kill(grupo, libc::SIGTERM) } == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
        if ya_no_esta {
            return false; // the whole group was already gone
        }
        let tope = std::time::Instant::now() + grace;
        while std::time::Instant::now() < tope {
            if hijo.try_wait().map(|t| t.is_some()).unwrap_or(true) {
                return true; // the courteous one was enough
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        unsafe { libc::kill(grupo, libc::SIGKILL) };
        let _ = hijo.wait(); // reaps the wrapper: no zombies
        true
    }
    #[cfg(windows)]
    {
        // Windows has no Unix signals and the only std termination hits
        // the direct child alone: the shim's grandchildren keep the pipe
        // open and the app stays hung. `taskkill /T /F` kills the tree.
        let pid = hijo.id().to_string();
        let _ = std::process::Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid])
            .status();
        let _ = hijo.wait();
        true
    }
}
