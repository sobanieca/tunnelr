//! Graceful stop on SIGINT and SIGTERM: the signals are blocked in every
//! thread and one thread waits for them.

/// Blocks SIGINT and SIGTERM. Call before starting other threads, they
/// inherit the signal mask.
pub fn block() {
    // SAFETY: the set is initialized by sigemptyset before use.
    unsafe {
        let set = signal_set();
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
    }
}

unsafe fn signal_set() -> libc::sigset_t {
    // SAFETY: sigset_t is plain data, initialized by sigemptyset.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGINT);
        libc::sigaddset(&mut set, libc::SIGTERM);
        set
    }
}

/// Runs `handler` in a separate thread on the first SIGINT or SIGTERM.
/// `block` must have been called first.
pub fn on_terminate(handler: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        let mut signal = 0;
        // SAFETY: waits on the blocked signals, writes only to `signal`.
        unsafe {
            let set = signal_set();
            libc::sigwait(&set, &mut signal);
        }
        handler();
    });
}
