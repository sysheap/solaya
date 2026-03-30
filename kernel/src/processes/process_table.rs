use alloc::{
    collections::{BTreeMap, VecDeque},
    vec::Vec,
};
use common::pid::Tid;
use core::{
    sync::atomic::{AtomicUsize, Ordering},
    task::Waker,
};

use crate::{
    cpu::Cpu,
    debug, info,
    klibc::{Spinlock, elf::ElfFile, runtime_initialized::RuntimeInitializedData},
    processes::{futex, process::POWERSAVE_TID, thread::Thread},
};

use super::{
    thread::{ThreadRef, ThreadState},
    wait_child::WaitPid,
};

pub static RUN_QUEUE: Spinlock<VecDeque<ThreadRef>> = Spinlock::new(VecDeque::new());
static LIVE_THREAD_COUNT: AtomicUsize = AtomicUsize::new(0);

pub fn is_empty() -> bool {
    LIVE_THREAD_COUNT.load(Ordering::Relaxed) == 0
}

pub fn live_thread_count() -> usize {
    LIVE_THREAD_COUNT.load(Ordering::Relaxed)
}

pub static THE: RuntimeInitializedData<Spinlock<ProcessTable>> = RuntimeInitializedData::new();

pub fn init() {
    THE.initialize(Spinlock::new(ProcessTable::new()));
}

pub fn spawn_init(elf_data: &[u8]) {
    let elf = ElfFile::parse(elf_data).expect("Cannot parse /init ELF");
    let default_env = ["PATH=/bin", "HOME=/", "TERM=dumb", "PS1=$ "];
    let thread =
        Thread::from_elf(&elf, "init", &[], &default_env, Tid::new(0)).expect("init must succeed");
    THE.with_lock(|mut pt| pt.add_thread(thread));
}

pub struct ProcessTable {
    threads: BTreeMap<Tid, ThreadRef>,
    children: BTreeMap<Tid, Vec<Tid>>,
    wait_wakers: Vec<Waker>,
}

impl ProcessTable {
    pub fn new() -> Self {
        Self {
            threads: BTreeMap::new(),
            children: BTreeMap::new(),
            wait_wakers: Vec::new(),
        }
    }

    pub fn add_thread(&mut self, thread: ThreadRef) {
        let (tid, parent_tid) = thread.with_lock(|t| (t.get_tid(), t.parent_tid()));
        LIVE_THREAD_COUNT.fetch_add(1, Ordering::Relaxed);
        RUN_QUEUE.lock().push_back(thread.clone());
        assert!(
            self.threads.insert(tid, thread).is_none(),
            "Duplicate TID {tid} in process table"
        );
        self.children.entry(parent_tid).or_default().push(tid);
    }

    pub fn dump(&self) {
        for (tid, thread) in &self.threads {
            if let Some(()) = thread.try_with_lock(|t| {
                info!(
                    "  thread tid={tid} state={:?} name={}",
                    t.get_state(),
                    t.get_name()
                );
            }) {
            } else {
                info!("  thread tid={tid} (locked)");
            }
        }
    }

    pub fn take_zombie(&mut self, parent_tid: Tid, target: &WaitPid) -> Option<(Tid, i32)> {
        let children = self.children.get(&parent_tid)?;

        let tid = match target {
            WaitPid::Specific(wanted) => {
                if !children.contains(wanted) {
                    return None;
                }
                let is_zombie = self
                    .threads
                    .get(wanted)?
                    .with_lock(|t| matches!(t.get_state(), ThreadState::Zombie(_)));
                if !is_zombie {
                    return None;
                }
                *wanted
            }
            WaitPid::Any => *children.iter().find(|&&child_tid| {
                self.threads.get(&child_tid).is_some_and(|t| {
                    t.with_lock(|t| matches!(t.get_state(), ThreadState::Zombie(_)))
                })
            })?,
            WaitPid::Pgid(pgid) => *children.iter().find(|&&child_tid| {
                self.threads.get(&child_tid).is_some_and(|t| {
                    t.with_lock(|t| {
                        matches!(t.get_state(), ThreadState::Zombie(_))
                            && t.process().lock().pgid() == *pgid
                    })
                })
            })?,
        };

        let thread = self.threads.remove(&tid).expect("tid was just found");
        let wstatus = thread.with_lock(|t| match t.get_state() {
            ThreadState::Zombie(exit_status) => exit_status.to_wstatus(),
            _ => unreachable!(),
        });

        if let Some(children) = self.children.get_mut(&parent_tid) {
            children.retain(|c| *c != tid);
        }
        self.children.remove(&tid);

        Some((tid, wstatus))
    }

    pub fn take_stopped(&mut self, parent_tid: Tid, target: &WaitPid) -> Option<(Tid, i32)> {
        let children = self.children.get(&parent_tid)?;

        let tid = match target {
            WaitPid::Specific(wanted) => {
                if !children.contains(wanted) {
                    return None;
                }
                let is_stopped = self
                    .threads
                    .get(wanted)?
                    .with_lock(|t| t.get_state() == ThreadState::Stopped && !t.stopped_notified);
                if !is_stopped {
                    return None;
                }
                *wanted
            }
            WaitPid::Any => *children.iter().find(|&&child_tid| {
                self.threads.get(&child_tid).is_some_and(|t| {
                    t.with_lock(|t| t.get_state() == ThreadState::Stopped && !t.stopped_notified)
                })
            })?,
            WaitPid::Pgid(pgid) => *children.iter().find(|&&child_tid| {
                self.threads.get(&child_tid).is_some_and(|t| {
                    t.with_lock(|t| {
                        t.get_state() == ThreadState::Stopped
                            && !t.stopped_notified
                            && t.process().lock().pgid() == *pgid
                    })
                })
            })?,
        };

        let thread = self.threads.get(&tid).expect("tid was just found");
        let stop_sig = thread.with_lock(|mut t| {
            t.stopped_notified = true;
            t.stop_signal
        });

        // WIFSTOPPED encoding: (stop_signal << 8) | 0x7f
        let wstatus =
            (i32::from(u8::try_from(stop_sig).expect("stop signal fits in u8")) << 8) | 0x7f;
        Some((tid, wstatus))
    }

    pub fn get_pgid_of(&self, tid: Tid) -> Option<Tid> {
        let thread = self.threads.get(&tid)?;
        Some(thread.with_lock(|t| t.process().lock().pgid()))
    }

    pub fn get_sid_of(&self, tid: Tid) -> Option<Tid> {
        let thread = self.threads.get(&tid)?;
        Some(thread.with_lock(|t| t.process().lock().sid()))
    }

    pub fn set_pgid_of(&mut self, tid: Tid, pgid: Tid) -> bool {
        if let Some(thread) = self.threads.get(&tid) {
            thread.with_lock(|t| t.process().lock().set_pgid(pgid));
            true
        } else {
            false
        }
    }

    pub fn is_child_of(&self, parent_tid: Tid, child_tid: Tid) -> bool {
        self.children
            .get(&parent_tid)
            .is_some_and(|c| c.contains(&child_tid))
    }

    pub fn has_any_child_of(&self, parent_tid: Tid) -> bool {
        self.children
            .get(&parent_tid)
            .is_some_and(|c| !c.is_empty())
    }

    pub fn register_wait_waker(&mut self, waker: Waker) {
        self.wait_wakers.push(waker);
    }

    pub fn get_thread(&self, tid: Tid) -> Option<&ThreadRef> {
        self.threads.get(&tid)
    }

    pub fn wake_wait_wakers(&mut self) {
        for waker in self.wait_wakers.drain(..) {
            waker.wake();
        }
    }

    pub fn send_signal(&mut self, tid: Tid, sig: u32) {
        if let Some(thread) = self.threads.get(&tid).cloned() {
            let should_enqueue = thread.with_lock(|mut t| {
                if matches!(t.get_state(), ThreadState::Zombie(_)) {
                    return false;
                }
                t.raise_signal(sig);
                // SIGCONT resumes stopped threads
                if sig == headers::syscall_types::SIGCONT && t.get_state() == ThreadState::Stopped {
                    t.clear_pending_stop_signals();
                    t.set_state(ThreadState::Runnable);
                    return true;
                }
                if t.has_pending_unblocked_signal() && t.get_state() == ThreadState::Waiting {
                    t.set_state(ThreadState::Runnable);
                    return true;
                }
                false
            });
            if should_enqueue {
                RUN_QUEUE.lock().push_back(thread);
            }
        }
    }

    pub fn send_signal_to_pgid(&mut self, pgid: Tid, sig: u32) {
        let tids: Vec<Tid> = self
            .threads
            .iter()
            .filter(|(_, t)| {
                t.with_lock(|t| {
                    !matches!(t.get_state(), ThreadState::Zombie(_))
                        && t.process().lock().pgid() == pgid
                })
            })
            .map(|(tid, _)| *tid)
            .collect();
        for tid in tids {
            self.send_signal(tid, sig);
        }
    }

    /// Sends signal to all threads in the process. Linux delivers process-directed
    /// signals to a single eligible thread; we simplify by raising on all.
    pub fn send_signal_to_process(&mut self, tid: Tid, sig: u32) {
        let Some(thread) = self.threads.get(&tid).cloned() else {
            return;
        };
        let all_tids = thread.lock().process().lock().thread_tids();
        for t in all_tids {
            self.send_signal(t, sig);
        }
    }

    pub fn kill_process_of(&mut self, tid: Tid, exit_status: super::signal::ExitStatus) {
        let Some(thread) = self.threads.get(&tid).cloned() else {
            return;
        };
        let all_tids = thread.lock().process().lock().thread_tids();
        for t in all_tids {
            self.kill(t, exit_status);
        }
    }

    pub fn kill(&mut self, tid: Tid, exit_status: super::signal::ExitStatus) {
        assert!(
            tid != POWERSAVE_TID,
            "We are not allowed to kill the never process"
        );
        debug!("Killing tid={tid}");
        if let Some(thread) = self.threads.get(&tid).cloned() {
            let already_dead =
                thread.with_lock(|t| matches!(t.get_state(), ThreadState::Zombie(_)));
            if already_dead {
                return;
            }
            LIVE_THREAD_COUNT.fetch_sub(1, Ordering::Relaxed);
            let (main_tid, futex_addr) = thread.with_lock(|mut t| {
                t.set_state(ThreadState::Zombie(exit_status));
                t.take_syscall_task();
                Cpu::current().ipi_to_all_but_me();

                let futex_addr = if let Some(clear_child_tid) = t.get_clear_child_tid() {
                    let process = t.process();
                    let addr = clear_child_tid.get() as usize;
                    let _ = clear_child_tid.write_with_process_lock(&mut process.lock(), 0);
                    Some(addr)
                } else {
                    None
                };

                let process = t.process();
                let mut p = process.lock();
                p.remove_thread(tid);
                if p.has_no_threads() {
                    p.close_all_fds();
                }
                (p.main_tid(), futex_addr)
            });

            if let Some(addr) = futex_addr {
                futex::futex_wake(main_tid, addr, 1);
            }

            self.wake_wait_wakers();

            if tid == main_tid {
                // Reparent orphans to init only when the main thread dies
                let init_tid = Tid::new(1);
                if let Some(orphans) = self.children.remove(&main_tid) {
                    for &child_tid in &orphans {
                        if let Some(child_thread) = self.threads.get(&child_tid) {
                            child_thread.with_lock(|mut t| {
                                t.set_parent_tid(init_tid);
                            });
                        }
                    }
                    self.children.entry(init_tid).or_default().extend(orphans);
                }
            }
        }
    }
}
