pub trait NeedsUserSpaceWrapper {
    type Wrapped;
    fn wrap_arg(value: usize, process: ProcessRef) -> Result<Self::Wrapped, headers::errno::Errno>;
}

macro_rules! impl_userspace_arg {
    ($type:ty) => {
        impl<T> NeedsUserSpaceWrapper for $type {
            type Wrapped = LinuxUserspaceArg<$type>;
            fn wrap_arg(
                value: usize,
                process: ProcessRef,
            ) -> Result<Self::Wrapped, headers::errno::Errno> {
                Ok(LinuxUserspaceArg::new(value, process))
            }
        }
    };
}

impl_userspace_arg!(*const T);
impl_userspace_arg!(*mut T);
impl_userspace_arg!(Option<*const T>);
impl_userspace_arg!(Option<*mut T>);

impl NeedsUserSpaceWrapper for c_int {
    type Wrapped = c_int;
    fn wrap_arg(
        value: usize,
        _process: ProcessRef,
    ) -> Result<Self::Wrapped, headers::errno::Errno> {
        c_int::try_from(value as isize).map_err(|_| headers::errno::Errno::EINVAL)
    }
}

impl NeedsUserSpaceWrapper for c_uint {
    type Wrapped = c_uint;
    fn wrap_arg(
        value: usize,
        _process: ProcessRef,
    ) -> Result<Self::Wrapped, headers::errno::Errno> {
        // Truncate to low 32 bits. On RISC-V 64, the ABI sign-extends 32-bit
        // values in 64-bit registers, so e.g. 0x80000002 becomes
        // 0xFFFFFFFF80000002. Truncation recovers the original value.
        Ok(value as c_uint)
    }
}

impl NeedsUserSpaceWrapper for c_ulong {
    type Wrapped = c_ulong;
    fn wrap_arg(
        value: usize,
        _process: ProcessRef,
    ) -> Result<Self::Wrapped, headers::errno::Errno> {
        Ok(value as c_ulong)
    }
}

impl NeedsUserSpaceWrapper for usize {
    type Wrapped = usize;
    fn wrap_arg(
        value: usize,
        _process: ProcessRef,
    ) -> Result<Self::Wrapped, headers::errno::Errno> {
        Ok(value)
    }
}

impl NeedsUserSpaceWrapper for isize {
    type Wrapped = isize;
    fn wrap_arg(
        value: usize,
        _process: ProcessRef,
    ) -> Result<Self::Wrapped, headers::errno::Errno> {
        Ok(value as isize)
    }
}

macro_rules! linux_syscalls {
    ($($number:ident => $name:ident ($($arg_name: ident: $arg_ty:ty),*);)*) => {
        use $crate::syscalls::linux_validator::LinuxUserspaceArg;
        pub trait LinuxSyscalls {
            $(async fn $name(&mut self, $($arg_name: <$arg_ty as $crate::syscalls::macros::NeedsUserSpaceWrapper>::Wrapped),*) -> Result<isize, headers::errno::Errno>;)*

            fn get_process(&self) -> $crate::processes::process::ProcessRef;

            async fn handle(&mut self, trap_frame: &TrapFrame) -> Result<isize, headers::errno::Errno> {
                let nr = trap_frame[Register::a7];
                let args = [
                    trap_frame[Register::a0],
                    trap_frame[Register::a1],
                    trap_frame[Register::a2],
                    trap_frame[Register::a3],
                    trap_frame[Register::a4],
                    trap_frame[Register::a5]
                ];
                match nr {
                    $(headers::syscalls::$number => self.$name($(<$arg_ty as $crate::syscalls::macros::NeedsUserSpaceWrapper>::wrap_arg(args[${index()}], self.get_process())?),*).await),*,
                    _ => {
                        $crate::syscalls::tracer::log_unimplemented_and_kill(trap_frame);
                        Err(headers::errno::Errno::ENOSYS)
                    }
                }
            }
        }

        pub const SYSCALL_METADATA: &[(usize, $crate::syscalls::tracer::SyscallMetadata)] = &[
            $(
                (headers::syscalls::$number, $crate::syscalls::tracer::SyscallMetadata {
                    name: stringify!($name),
                    args: &[
                        $((
                            stringify!($arg_name),
                            <$arg_ty as $crate::syscalls::tracer::SyscallArgFormat>::FORMAT,
                        )),*
                    ],
                }),
            )*
        ];
    };
}

use core::ffi::{c_int, c_uint, c_ulong};

pub(super) use linux_syscalls;

use crate::{processes::process::ProcessRef, syscalls::linux_validator::LinuxUserspaceArg};
