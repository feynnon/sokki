//! Core Audio process objects via raw FFI (CoreAudio + CoreFoundation are
//! system frameworks; `proc_pidpath` lives in libSystem).

use std::ffi::{c_void, CStr};
use std::os::raw::c_char;
use std::ptr;
use std::sync::OnceLock;

use crate::{display_name, AudioClient, Error, Result};

type AudioObjectID = u32;
type OSStatus = i32;
type CFStringRef = *const c_void;
type CFIndex = isize;

#[repr(C)]
struct AudioObjectPropertyAddress {
    selector: u32,
    scope: u32,
    element: u32,
}

const fn fourcc(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}

const SYSTEM_OBJECT: AudioObjectID = 1; // kAudioObjectSystemObject
const SCOPE_GLOBAL: u32 = fourcc(b"glob"); // kAudioObjectPropertyScopeGlobal
const ELEMENT_MAIN: u32 = 0; // kAudioObjectPropertyElementMain
const PROCESS_OBJECT_LIST: u32 = fourcc(b"prs#"); // kAudioHardwarePropertyProcessObjectList
const PROCESS_PID: u32 = fourcc(b"ppid"); // kAudioProcessPropertyPID
const PROCESS_BUNDLE_ID: u32 = fourcc(b"pbid"); // kAudioProcessPropertyBundleID
const PROCESS_IS_RUNNING_INPUT: u32 = fourcc(b"piri"); // kAudioProcessPropertyIsRunningInput

const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioObjectHasProperty(id: AudioObjectID, address: *const AudioObjectPropertyAddress) -> u8;
    fn AudioObjectGetPropertyDataSize(
        id: AudioObjectID,
        address: *const AudioObjectPropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        out_size: *mut u32,
    ) -> OSStatus;
    fn AudioObjectGetPropertyData(
        id: AudioObjectID,
        address: *const AudioObjectPropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        io_size: *mut u32,
        out_data: *mut c_void,
    ) -> OSStatus;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringGetLength(s: CFStringRef) -> CFIndex;
    fn CFStringGetMaximumSizeForEncoding(length: CFIndex, encoding: u32) -> CFIndex;
    fn CFStringGetCString(s: CFStringRef, buf: *mut c_char, size: CFIndex, encoding: u32) -> u8;
    fn CFRelease(cf: *const c_void);
}

fn addr(selector: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        selector,
        scope: SCOPE_GLOBAL,
        element: ELEMENT_MAIN,
    }
}

/// Read a fixed-size plain-data property.
fn get_scalar<T: Copy + Default>(id: AudioObjectID, selector: u32) -> Result<T> {
    let a = addr(selector);
    let mut value = T::default();
    let mut size = std::mem::size_of::<T>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            id,
            &a,
            0,
            ptr::null(),
            &mut size,
            &mut value as *mut T as *mut c_void,
        )
    };
    if status != 0 {
        return Err(Error::CoreAudio {
            call: "AudioObjectGetPropertyData",
            status,
        });
    }
    Ok(value)
}

fn process_object_ids() -> Result<Vec<AudioObjectID>> {
    let a = addr(PROCESS_OBJECT_LIST);
    if unsafe { AudioObjectHasProperty(SYSTEM_OBJECT, &a) } == 0 {
        return Err(Error::Unsupported);
    }
    // The list can grow between the size query and the read; retry a few
    // times with the size the HAL reports.
    for _ in 0..4 {
        let mut size: u32 = 0;
        let status =
            unsafe { AudioObjectGetPropertyDataSize(SYSTEM_OBJECT, &a, 0, ptr::null(), &mut size) };
        if status != 0 {
            return Err(Error::CoreAudio {
                call: "AudioObjectGetPropertyDataSize",
                status,
            });
        }
        let count = size as usize / std::mem::size_of::<AudioObjectID>();
        // One slot of headroom so a list that grew by one is noticed.
        let mut ids: Vec<AudioObjectID> = vec![0; count + 1];
        let mut io_size = (ids.len() * std::mem::size_of::<AudioObjectID>()) as u32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                SYSTEM_OBJECT,
                &a,
                0,
                ptr::null(),
                &mut io_size,
                ids.as_mut_ptr() as *mut c_void,
            )
        };
        if status != 0 {
            return Err(Error::CoreAudio {
                call: "AudioObjectGetPropertyData",
                status,
            });
        }
        let got = io_size as usize / std::mem::size_of::<AudioObjectID>();
        if got <= count {
            ids.truncate(got);
            return Ok(ids);
        }
    }
    Err(Error::CoreAudio {
        call: "AudioObjectGetPropertyData (process list kept changing)",
        status: 0,
    })
}

fn process_bundle_id(id: AudioObjectID) -> Option<String> {
    let a = addr(PROCESS_BUNDLE_ID);
    let mut s: CFStringRef = ptr::null();
    let mut size = std::mem::size_of::<CFStringRef>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            id,
            &a,
            0,
            ptr::null(),
            &mut size,
            &mut s as *mut CFStringRef as *mut c_void,
        )
    };
    if status != 0 || s.is_null() {
        return None;
    }
    // "The caller is responsible for releasing the returned CFObject."
    let out = cfstring_to_string(s);
    unsafe { CFRelease(s) };
    out.filter(|b| !b.is_empty())
}

fn cfstring_to_string(s: CFStringRef) -> Option<String> {
    unsafe {
        let len = CFStringGetLength(s);
        let cap = CFStringGetMaximumSizeForEncoding(len, CF_STRING_ENCODING_UTF8) + 1;
        if cap <= 0 {
            return None;
        }
        let mut buf = vec![0 as c_char; cap as usize];
        if CFStringGetCString(s, buf.as_mut_ptr(), cap, CF_STRING_ENCODING_UTF8) == 0 {
            return None;
        }
        Some(CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned())
    }
}

/// Executable path for a pid (libproc, part of libSystem).
pub(crate) fn pid_executable_path(pid: u32) -> Option<String> {
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let len = unsafe {
        libc::proc_pidpath(
            pid as i32,
            buf.as_mut_ptr() as *mut c_void,
            buf.len() as u32,
        )
    };
    if len <= 0 {
        return None;
    }
    buf.truncate(len as usize);
    String::from_utf8(buf).ok()
}

type ResponsibleFn = unsafe extern "C" fn(libc::pid_t) -> libc::pid_t;

/// `responsibility_get_pid_responsible_for_pid` is a private libSystem
/// function (the one TCC uses to attribute an XPC helper's mic use to its
/// app). Looked up at runtime so a missing symbol degrades to `None`
/// instead of failing to link.
fn responsible_fn() -> Option<ResponsibleFn> {
    static FN: OnceLock<Option<ResponsibleFn>> = OnceLock::new();
    *FN.get_or_init(|| {
        let name = c"responsibility_get_pid_responsible_for_pid";
        let sym = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
        if sym.is_null() {
            None
        } else {
            Some(unsafe { std::mem::transmute::<*mut c_void, ResponsibleFn>(sym) })
        }
    })
}

/// The pid macOS holds responsible for `pid`, if it is a different process.
pub(crate) fn responsible_pid(pid: u32) -> Option<u32> {
    let f = responsible_fn()?;
    let r = unsafe { f(pid as libc::pid_t) };
    (r > 0 && r as u32 != pid).then_some(r as u32)
}

pub(crate) fn audio_input_clients() -> Result<Vec<AudioClient>> {
    let mut clients = Vec::new();
    for id in process_object_ids()? {
        // A process can disconnect between the list read and these reads;
        // skip it rather than failing the whole poll.
        let running: u32 = match get_scalar(id, PROCESS_IS_RUNNING_INPUT) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if running == 0 {
            continue;
        }
        let pid: i32 = match get_scalar(id, PROCESS_PID) {
            Ok(p) if p > 0 => p,
            _ => continue,
        };
        let pid = pid as u32;
        let bundle_id = process_bundle_id(id);
        let exe_path = pid_executable_path(pid);
        let name = display_name(pid, bundle_id.as_deref(), exe_path.as_deref());
        let responsible_pid = responsible_pid(pid);
        let responsible_path = responsible_pid.and_then(pid_executable_path);
        clients.push(AudioClient {
            pid,
            bundle_id,
            exe_path,
            name,
            responsible_pid,
            responsible_path,
        });
    }
    Ok(clients)
}
