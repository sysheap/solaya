use alloc::{string::String, vec::Vec};
use core::ffi::{c_int, c_uint, c_ulong};
use headers::{
    errno::Errno,
    syscall_types::{FUTEX_PRIVATE_FLAG, FUTEX_WAIT, FUTEX_WAKE, PR_GET_NAME, PR_SET_NAME},
};

use crate::processes::{
    futex::{self, FutexWait},
    process_table,
};
use common::pid::Tid;

use super::{linux::LinuxSyscallHandler, linux_validator::LinuxUserspaceArg};

impl LinuxSyscallHandler {
    pub(super) fn do_getpgid(&self, pid: c_int) -> Result<isize, Errno> {
        if pid == 0 {
            let pgid = self.current_process.with_lock(|p| p.pgid());
            return Ok(pgid.as_isize());
        }
        let target = Tid::try_from_i32(pid).ok_or(Errno::ESRCH)?;
        let pgid = process_table::THE
            .lock()
            .get_pgid_of(target)
            .ok_or(Errno::ESRCH)?;
        Ok(pgid.as_isize())
    }

    pub(super) fn do_getsid(&self, pid: c_int) -> Result<isize, Errno> {
        if pid == 0 {
            let sid = self.current_process.with_lock(|p| p.sid());
            return Ok(sid.as_isize());
        }
        let target = Tid::try_from_i32(pid).ok_or(Errno::ESRCH)?;
        let sid = process_table::THE
            .lock()
            .get_sid_of(target)
            .ok_or(Errno::ESRCH)?;
        Ok(sid.as_isize())
    }

    pub(super) fn do_setpgid(&self, pid: c_int, pgid: c_int) -> Result<isize, Errno> {
        let my_main_tid = self.current_process.with_lock(|p| p.main_tid());
        let target_tid = if pid == 0 {
            my_main_tid
        } else {
            Tid::try_from_i32(pid).ok_or(Errno::EINVAL)?
        };
        let new_pgid = if pgid == 0 {
            target_tid
        } else {
            Tid::try_from_i32(pgid).ok_or(Errno::EINVAL)?
        };

        if target_tid != my_main_tid {
            let is_child = process_table::THE
                .lock()
                .is_child_of(my_main_tid, target_tid);
            if !is_child {
                return Err(Errno::ESRCH);
            }
        }

        if !process_table::THE.lock().set_pgid_of(target_tid, new_pgid) {
            return Err(Errno::ESRCH);
        }
        Ok(0)
    }

    pub(super) fn do_setuid(&self, uid: c_uint) -> Result<isize, Errno> {
        self.current_process.with_lock(|mut p| {
            let cred = p.credentials_mut();
            cred.uid = uid;
            cred.euid = uid;
            cred.suid = uid;
        });
        Ok(0)
    }

    pub(super) fn do_setgid(&self, gid: c_uint) -> Result<isize, Errno> {
        self.current_process.with_lock(|mut p| {
            let cred = p.credentials_mut();
            cred.gid = gid;
            cred.egid = gid;
            cred.sgid = gid;
        });
        Ok(0)
    }

    pub(super) fn do_setreuid(&self, ruid: c_uint, euid: c_uint) -> Result<isize, Errno> {
        self.current_process.with_lock(|mut p| {
            let cred = p.credentials_mut();
            if ruid != u32::MAX {
                cred.uid = ruid;
            }
            if euid != u32::MAX {
                cred.euid = euid;
            }
        });
        Ok(0)
    }

    pub(super) fn do_setregid(&self, rgid: c_uint, egid: c_uint) -> Result<isize, Errno> {
        self.current_process.with_lock(|mut p| {
            let cred = p.credentials_mut();
            if rgid != u32::MAX {
                cred.gid = rgid;
            }
            if egid != u32::MAX {
                cred.egid = egid;
            }
        });
        Ok(0)
    }

    pub(super) fn do_setresuid(
        &self,
        ruid: c_uint,
        euid: c_uint,
        suid: c_uint,
    ) -> Result<isize, Errno> {
        self.current_process.with_lock(|mut p| {
            let cred = p.credentials_mut();
            if ruid != u32::MAX {
                cred.uid = ruid;
            }
            if euid != u32::MAX {
                cred.euid = euid;
            }
            if suid != u32::MAX {
                cred.suid = suid;
            }
        });
        Ok(0)
    }

    pub(super) fn do_setresgid(
        &self,
        rgid: c_uint,
        egid: c_uint,
        sgid: c_uint,
    ) -> Result<isize, Errno> {
        self.current_process.with_lock(|mut p| {
            let cred = p.credentials_mut();
            if rgid != u32::MAX {
                cred.gid = rgid;
            }
            if egid != u32::MAX {
                cred.egid = egid;
            }
            if sgid != u32::MAX {
                cred.sgid = sgid;
            }
        });
        Ok(0)
    }

    pub(super) fn do_getresuid(
        &self,
        ruid: LinuxUserspaceArg<*mut u8>,
        euid: LinuxUserspaceArg<*mut u8>,
        suid: LinuxUserspaceArg<*mut u8>,
    ) -> Result<isize, Errno> {
        let (r, e, s) = self.current_process.with_lock(|p| {
            let c = p.credentials();
            (c.uid, c.euid, c.suid)
        });
        ruid.write_slice(&r.to_le_bytes())?;
        euid.write_slice(&e.to_le_bytes())?;
        suid.write_slice(&s.to_le_bytes())?;
        Ok(0)
    }

    pub(super) fn do_getresgid(
        &self,
        rgid: LinuxUserspaceArg<*mut u8>,
        egid: LinuxUserspaceArg<*mut u8>,
        sgid: LinuxUserspaceArg<*mut u8>,
    ) -> Result<isize, Errno> {
        let (r, e, s) = self.current_process.with_lock(|p| {
            let c = p.credentials();
            (c.gid, c.egid, c.sgid)
        });
        rgid.write_slice(&r.to_le_bytes())?;
        egid.write_slice(&e.to_le_bytes())?;
        sgid.write_slice(&s.to_le_bytes())?;
        Ok(0)
    }

    pub(super) fn do_getgroups(
        &self,
        size: c_int,
        list: LinuxUserspaceArg<*mut u8>,
    ) -> Result<isize, Errno> {
        let groups = self
            .current_process
            .with_lock(|p| p.credentials().groups.clone());
        if size == 0 {
            return Ok(groups.len() as isize);
        }
        let size: usize = size.try_into().map_err(|_| Errno::EINVAL)?;
        if size < groups.len() {
            return Err(Errno::EINVAL);
        }
        let bytes: Vec<u8> = groups.iter().flat_map(|g| g.to_le_bytes()).collect();
        if !bytes.is_empty() {
            list.write_slice(&bytes)?;
        }
        Ok(groups.len() as isize)
    }

    pub(super) fn do_setgroups(
        &self,
        size: c_int,
        list: LinuxUserspaceArg<*const u8>,
    ) -> Result<isize, Errno> {
        let groups = if size > 0 {
            let count: usize = size.try_into().map_err(|_| Errno::EINVAL)?;
            let bytes = list.validate_slice(count * 4)?;
            bytes
                .chunks_exact(4)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect()
        } else {
            Vec::new()
        };
        self.current_process
            .with_lock(|mut p| p.credentials_mut().groups = groups);
        Ok(0)
    }

    pub(super) fn do_prctl(
        &self,
        option: c_int,
        arg2: c_ulong,
        _arg3: c_ulong,
        _arg4: c_ulong,
        _arg5: c_ulong,
    ) -> Result<isize, Errno> {
        match option.cast_unsigned() {
            PR_SET_NAME => {
                let ptr = LinuxUserspaceArg::<*const u8>::new(
                    arg2 as usize,
                    self.current_process.clone(),
                );
                let name_bytes = ptr.validate_slice(16)?;
                let end = name_bytes
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(15)
                    .min(15);
                let name = core::str::from_utf8(&name_bytes[..end]).map_err(|_| Errno::EINVAL)?;
                self.current_thread
                    .with_lock(|mut t| t.set_thread_name(String::from(name)));
                Ok(0)
            }
            PR_GET_NAME => {
                let name = self.current_thread.with_lock(|t| {
                    t.thread_name()
                        .map(String::from)
                        .unwrap_or_else(|| String::from(t.process_name()))
                });
                let mut buf = [0u8; 16];
                let bytes = name.as_bytes();
                let len = bytes.len().min(15);
                buf[..len].copy_from_slice(&bytes[..len]);
                let ptr =
                    LinuxUserspaceArg::<*mut u8>::new(arg2 as usize, self.current_process.clone());
                ptr.write_slice(&buf)?;
                Ok(0)
            }
            _ => Err(Errno::EINVAL),
        }
    }

    pub(super) async fn do_futex(
        &self,
        uaddr: usize,
        op: c_int,
        val: c_uint,
    ) -> Result<isize, Errno> {
        let cmd = op & !(FUTEX_PRIVATE_FLAG as c_int);
        let main_tid = self.current_process.with_lock(|p| p.main_tid());
        match cmd.cast_unsigned() {
            FUTEX_WAIT => {
                let result =
                    FutexWait::new(self.current_process.clone(), uaddr, val, main_tid).await;
                Ok(result as isize)
            }
            FUTEX_WAKE => {
                let result = futex::futex_wake(main_tid, uaddr, val);
                Ok(result as isize)
            }
            _ => Err(Errno::ENOSYS),
        }
    }
}
