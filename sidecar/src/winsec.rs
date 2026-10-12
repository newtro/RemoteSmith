//! Windows security helpers shared by the client (server-identity check) and the reference
//! server (user-only pipe DACL). docs/IPC.md §2.

use std::ffi::c_void;
use std::io;
use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
    TOKEN_USER,
};
use windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};

/// String SID of the user owning `process` (a process handle with query rights).
fn process_user_sid(process: HANDLE) -> io::Result<String> {
    // SAFETY: standard token query sequence. The TOKEN_USER buffer is u64-backed so it is
    // suitably aligned; every handle / allocation is released before returning.
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut len = 0u32;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len);
        let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
        let ok = GetTokenInformation(
            token,
            TokenUser,
            buf.as_mut_ptr() as *mut c_void,
            (buf.len() * 8) as u32,
            &mut len,
        );
        CloseHandle(token);
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut wide: *mut u16 = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut wide) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut n = 0;
        while *wide.add(n) != 0 {
            n += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(wide, n));
        LocalFree(wide as _);
        Ok(s)
    }
}

/// String SID of the current process user.
pub fn current_user_sid() -> io::Result<String> {
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no closing.
    process_user_sid(unsafe { GetCurrentProcess() })
}

/// Verify that the server end of the connected pipe runs as the current user, so a
/// process that squatted the pipe name cannot impersonate the fork.
pub fn check_pipe_server_is_us(pipe: &impl std::os::windows::io::AsRawHandle) -> io::Result<()> {
    let mut pid = 0u32;
    // SAFETY: `pipe` borrows a live, connected pipe handle for the duration of the call.
    if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle() as HANDLE, &mut pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: plain FFI; the handle is closed below.
    let proc = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if proc.is_null() {
        return Err(io::Error::last_os_error());
    }
    let theirs = process_user_sid(proc);
    // SAFETY: `proc` was opened above.
    unsafe { CloseHandle(proc) };
    let (theirs, ours) = (theirs?, current_user_sid()?);
    if theirs != ours {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("pipe server runs as {theirs}, not {ours}; refusing"),
        ));
    }
    Ok(())
}

/// SECURITY_ATTRIBUTES granting GENERIC_ALL to the current user only. Leaked on purpose: it
/// must outlive every pipe instance created from it.
pub fn user_only_security_attributes() -> io::Result<&'static mut SECURITY_ATTRIBUTES> {
    let sddl = format!("D:P(A;;GA;;;{})", current_user_sid()?);
    let wide: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
    let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: `wide` is NUL-terminated; on success `sd` is a LocalAlloc'd descriptor we keep.
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide.as_ptr(),
            SDDL_REVISION_1,
            &mut sd,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Box::leak(Box::new(SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd,
        bInheritHandle: 0,
    })))
}
