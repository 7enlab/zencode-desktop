//! Real terminal width, std-only FFI (no external crates).
//! Unix: ioctl(TIOCGWINSZ); Windows: GetConsoleScreenBufferInfo.

/// Columns of the attached terminal (stdout, then stderr, then stdin), if any.
pub fn terminal_columns() -> Option<usize> {
    imp::columns().filter(|&c| c > 0)
}

#[cfg(unix)]
mod imp {
    use std::os::raw::{c_int, c_ulong, c_ushort};
    #[repr(C)]
    struct WinSize {
        row: c_ushort,
        col: c_ushort,
        xpixel: c_ushort,
        ypixel: c_ushort,
    }
    extern "C" {
        fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const TIOCGWINSZ: c_ulong = 0x5413;
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    const TIOCGWINSZ: c_ulong = 0x4008_7468; // macOS / BSD
    pub fn columns() -> Option<usize> {
        for fd in [1, 2, 0] {
            let mut ws = WinSize {
                row: 0,
                col: 0,
                xpixel: 0,
                ypixel: 0,
            };
            // SAFETY: ioctl writes at most size_of::<WinSize>() bytes into ws.
            let r = unsafe { ioctl(fd, TIOCGWINSZ, &mut ws as *mut WinSize) };
            if r == 0 && ws.col > 0 {
                return Some(ws.col as usize);
            }
        }
        None
    }
}

#[cfg(windows)]
mod imp {
    use std::os::raw::{c_int, c_short, c_ushort, c_void};
    #[repr(C)]
    #[derive(Default)]
    struct Coord {
        x: c_short,
        y: c_short,
    }
    #[repr(C)]
    #[derive(Default)]
    struct SmallRect {
        left: c_short,
        top: c_short,
        right: c_short,
        bottom: c_short,
    }
    #[repr(C)]
    #[derive(Default)]
    struct Csbi {
        size: Coord,
        cursor: Coord,
        attrs: c_ushort,
        window: SmallRect,
        max: Coord,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(n: u32) -> *mut c_void;
        fn GetConsoleScreenBufferInfo(h: *mut c_void, info: *mut Csbi) -> c_int;
    }
    pub fn columns() -> Option<usize> {
        for n in [-11i32 as u32, -12i32 as u32] {
            // STD_OUTPUT_HANDLE, STD_ERROR_HANDLE
            let mut info = Csbi::default();
            // SAFETY: plain Win32 calls with a valid out-pointer.
            let ok = unsafe { GetConsoleScreenBufferInfo(GetStdHandle(n), &mut info) };
            if ok != 0 {
                let w = info.window.right as i32 - info.window.left as i32 + 1;
                if w > 0 {
                    return Some(w as usize);
                }
            }
        }
        None
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    pub fn columns() -> Option<usize> {
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn does_not_panic() {
        let _ = super::terminal_columns();
    }
}
