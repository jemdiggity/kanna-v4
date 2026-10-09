mod ffi;

use std::ffi::c_void;
use std::fmt;
use std::ptr::{self, NonNull};

pub const GHOSTTY_COMMIT: &str = env!("KANNA_GHOSTTY_VT_COMMIT");
pub const MAX_UNFINISHED_SEQUENCE_BYTES: usize = 1 << 20;

const MAX_PENDING_REPLY_BYTES: usize = 1 << 20;

const DA1_FEATURES: [u16; 2] = [22, 52];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error {
    pub op: &'static str,
    pub code: i32,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.code {
            ffi::GHOSTTY_OUT_OF_MEMORY => "out of memory",
            ffi::GHOSTTY_INVALID_VALUE => "invalid value",
            ffi::GHOSTTY_OUT_OF_SPACE => "out of space",
            ffi::GHOSTTY_NO_VALUE => "no value",
            -5 => "I/O error",
            -6 => "limit exceeded",
            -7 => "rejected",
            _ => "error",
        };
        write!(f, "{} failed: {what} ({})", self.op, self.code)
    }
}

impl std::error::Error for Error {}

fn check(op: &'static str, code: ffi::GhosttyResult) -> Result<(), Error> {
    if code == ffi::GHOSTTY_SUCCESS {
        Ok(())
    } else {
        Err(Error { op, code })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lines {
    Screen,
    Last(usize),
}

pub struct Terminal {
    raw: NonNull<ffi::GhosttyTerminalImpl>,
    hooks: NonNull<Hooks>,
}

#[derive(Default)]
struct Hooks {
    replies: Vec<u8>,
    size: ffi::GhosttySizeReportSize,
}

unsafe impl Send for Terminal {}

impl Terminal {
    pub fn new(cols: u16, rows: u16, scrollback_lines: usize) -> Result<Terminal, Error> {
        let mut raw: ffi::GhosttyTerminal = ptr::null_mut();
        check("ghostty_terminal_new", unsafe {
            ffi::ghostty_terminal_new(ptr::null(), &mut raw, cols.max(1), rows.max(1))
        })?;
        let mut terminal = Terminal::adopt(raw)?;
        terminal.set_scrollback_bytes(None)?;
        terminal.set_scrollback_lines(scrollback_lines)?;
        let limit = MAX_UNFINISHED_SEQUENCE_BYTES;
        terminal.set("continuation limit", ffi::GHOSTTY_TERMINAL_OPT_CONTINUATION_MAX_BYTES, &limit)?;
        Ok(terminal)
    }

    pub fn from_snapshot(bytes: &[u8]) -> Result<Terminal, Error> {
        let mut decoder: ffi::GhosttySnapshotDecoder = ptr::null_mut();
        check("ghostty_snapshot_decoder_new_buf", unsafe {
            ffi::ghostty_snapshot_decoder_new_buf(ptr::null(), &mut decoder, bytes.as_ptr(), bytes.len())
        })?;
        let mut raw: ffi::GhosttyTerminal = ptr::null_mut();
        let limit = MAX_UNFINISHED_SEQUENCE_BYTES;
        let retain = true;
        let result = unsafe {
            check(
                "snapshot continuation limit",
                ffi::ghostty_snapshot_decoder_set(
                    decoder,
                    ffi::GHOSTTY_SNAPSHOT_DECODER_OPT_MAX_CONTINUATION_BYTES,
                    &limit as *const usize as *const c_void,
                ),
            )
            .and_then(|()| {
                check(
                    "snapshot retain continuation",
                    ffi::ghostty_snapshot_decoder_set(
                        decoder,
                        ffi::GHOSTTY_SNAPSHOT_DECODER_OPT_RETAIN_CONTINUATION,
                        &retain as *const bool as *const c_void,
                    ),
                )
            })
            .and_then(|()| check("ghostty_snapshot_decoder_decode", ffi::ghostty_snapshot_decoder_decode(decoder, &mut raw)))
        };
        unsafe { ffi::ghostty_snapshot_decoder_free(decoder) };
        result?;
        Terminal::adopt(raw)
    }

    fn adopt(raw: ffi::GhosttyTerminal) -> Result<Terminal, Error> {
        let raw = NonNull::new(raw).ok_or(Error { op: "ghostty_terminal_new", code: ffi::GHOSTTY_INVALID_VALUE })?;
        let hooks = NonNull::from(Box::leak(Box::new(Hooks::default())));
        let mut terminal = Terminal { raw, hooks };
        let (columns, rows) = terminal.size();
        terminal.hooks().size = ffi::GhosttySizeReportSize { rows, columns, cell_width: 0, cell_height: 0 };
        let userdata: *mut Hooks = hooks.as_ptr();
        let write: ffi::GhosttyTerminalWritePtyFn = write_pty;
        let attributes: ffi::GhosttyTerminalDeviceAttributesFn = device_attributes;
        let size: ffi::GhosttyTerminalSizeFn = size_report;
        unsafe {
            check(
                "terminal userdata",
                ffi::ghostty_terminal_set(raw.as_ptr(), ffi::GHOSTTY_TERMINAL_OPT_USERDATA, userdata as *const c_void),
            )?;
            check(
                "terminal write_pty",
                ffi::ghostty_terminal_set(raw.as_ptr(), ffi::GHOSTTY_TERMINAL_OPT_WRITE_PTY, write as *const c_void),
            )?;
            check(
                "terminal device_attributes",
                ffi::ghostty_terminal_set(raw.as_ptr(), ffi::GHOSTTY_TERMINAL_OPT_DEVICE_ATTRIBUTES, attributes as *const c_void),
            )?;
            check("terminal size", ffi::ghostty_terminal_set(raw.as_ptr(), ffi::GHOSTTY_TERMINAL_OPT_SIZE, size as *const c_void))?;
        }
        Ok(terminal)
    }

    fn hooks(&mut self) -> &mut Hooks {
        unsafe { &mut *self.hooks.as_ptr() }
    }

    fn set<T>(&mut self, op: &'static str, option: i32, value: &T) -> Result<(), Error> {
        check(op, unsafe { ffi::ghostty_terminal_set(self.raw.as_ptr(), option, value as *const T as *const c_void) })
    }

    fn get<T: Default>(&self, data: i32) -> Option<T> {
        let mut out = T::default();
        let code = unsafe { ffi::ghostty_terminal_get(self.raw.as_ptr(), data, &mut out as *mut T as *mut c_void) };
        (code == ffi::GHOSTTY_SUCCESS).then_some(out)
    }

    fn get_string(&self, data: i32) -> String {
        let mut out = ffi::GhosttyString { ptr: ptr::null(), len: 0 };
        let code = unsafe { ffi::ghostty_terminal_get(self.raw.as_ptr(), data, &mut out as *mut _ as *mut c_void) };
        if code != ffi::GHOSTTY_SUCCESS || out.ptr.is_null() || out.len == 0 {
            return String::new();
        }
        String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(out.ptr, out.len) }).into_owned()
    }

    pub fn write(&mut self, bytes: &[u8]) {
        if !bytes.is_empty() {
            unsafe { ffi::ghostty_terminal_vt_write(self.raw.as_ptr(), bytes.as_ptr(), bytes.len()) };
        }
    }

    pub fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.hooks().replies)
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), Error> {
        let (cols, rows) = (cols.max(1), rows.max(1));
        let size = self.hooks().size;
        check("ghostty_terminal_resize", unsafe {
            ffi::ghostty_terminal_resize(self.raw.as_ptr(), cols, rows, size.cell_width, size.cell_height)
        })?;
        (self.hooks().size.columns, self.hooks().size.rows) = (cols, rows);
        Ok(())
    }

    pub fn set_cell_pixels(&mut self, width: u32, height: u32) {
        (self.hooks().size.cell_width, self.hooks().size.cell_height) = (width, height);
    }

    pub fn set_scrollback_lines(&mut self, lines: usize) -> Result<(), Error> {
        self.set("scrollback limit", ffi::GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_LINES, &lines)
    }

    pub fn set_scrollback_bytes(&mut self, bytes: Option<usize>) -> Result<(), Error> {
        let value = bytes.as_ref().map_or(ptr::null(), |b| b as *const usize as *const c_void);
        check("scrollback byte limit", unsafe {
            ffi::ghostty_terminal_set(self.raw.as_ptr(), ffi::GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES, value)
        })
    }

    pub fn scrollback_bytes(&self) -> Option<usize> {
        self.get(ffi::GHOSTTY_TERMINAL_DATA_SCROLLBACK_MAX_BYTES)
    }

    pub fn scrollback_lines(&self) -> Option<usize> {
        self.get(ffi::GHOSTTY_TERMINAL_DATA_SCROLLBACK_MAX_LINES)
    }

    pub fn size(&self) -> (u16, u16) {
        (self.get(ffi::GHOSTTY_TERMINAL_DATA_COLS).unwrap_or(0), self.get(ffi::GHOSTTY_TERMINAL_DATA_ROWS).unwrap_or(0))
    }

    pub fn cursor(&self) -> (u16, u16) {
        (
            self.get(ffi::GHOSTTY_TERMINAL_DATA_CURSOR_X).unwrap_or(0),
            self.get(ffi::GHOSTTY_TERMINAL_DATA_CURSOR_Y).unwrap_or(0),
        )
    }

    pub fn cursor_visible(&self) -> bool {
        self.get(ffi::GHOSTTY_TERMINAL_DATA_CURSOR_VISIBLE).unwrap_or(true)
    }

    pub fn alternate_screen(&self) -> bool {
        self.get::<i32>(ffi::GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN) == Some(ffi::GHOSTTY_TERMINAL_SCREEN_ALTERNATE)
    }

    pub fn title(&self) -> String {
        self.get_string(ffi::GHOSTTY_TERMINAL_DATA_TITLE)
    }

    pub fn pwd(&self) -> String {
        self.get_string(ffi::GHOSTTY_TERMINAL_DATA_PWD)
    }

    pub fn scrollback_rows(&self) -> usize {
        self.get(ffi::GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS).unwrap_or(0)
    }

    pub fn text(&self, lines: Lines) -> String {
        let total: usize = self.get(ffi::GHOSTTY_TERMINAL_DATA_TOTAL_ROWS).unwrap_or(0);
        let (cols, rows) = self.size();
        if total == 0 || cols == 0 || rows == 0 {
            return String::new();
        }
        let start = match lines {
            Lines::Screen => total.saturating_sub(rows as usize),
            Lines::Last(n) => total.saturating_sub(n.saturating_add(rows as usize)),
        };
        let mut out: Vec<String> = match self.format_rows(start, total - 1, cols - 1) {
            Some(text) => text.split('\n').map(|line| line.trim_end().to_string()).collect(),
            None => return String::new(),
        };
        while out.last().is_some_and(String::is_empty) {
            out.pop();
        }
        if let Lines::Last(n) = lines {
            out.drain(..out.len().saturating_sub(n));
        }
        out.join("\n")
    }

    fn format_rows(&self, first: usize, last: usize, last_col: u16) -> Option<String> {
        let point = |x: u16, y: usize| ffi::GhosttyPoint {
            tag: ffi::GHOSTTY_POINT_TAG_SCREEN,
            value: ffi::GhosttyPointValue { coordinate: ffi::GhosttyPointCoordinate { x, y: y.min(u32::MAX as usize) as u32 } },
        };
        let blank = ffi::GhosttyGridRef { size: std::mem::size_of::<ffi::GhosttyGridRef>(), node: ptr::null_mut(), x: 0, y: 0 };
        let (mut start, mut end) = (blank, blank);
        unsafe {
            if ffi::ghostty_terminal_grid_ref(self.raw.as_ptr(), point(0, first), &mut start) != ffi::GHOSTTY_SUCCESS
                || ffi::ghostty_terminal_grid_ref(self.raw.as_ptr(), point(last_col, last), &mut end) != ffi::GHOSTTY_SUCCESS
            {
                return None;
            }
        }
        let selection =
            ffi::GhosttySelection { size: std::mem::size_of::<ffi::GhosttySelection>(), start, end, rectangle: false };
        let options = ffi::GhosttyTerminalSelectionFormatOptions {
            size: std::mem::size_of::<ffi::GhosttyTerminalSelectionFormatOptions>(),
            emit: ffi::GHOSTTY_FORMATTER_FORMAT_PLAIN,
            unwrap: false,
            trim: true,
            selection: &selection,
        };
        let mut out: *mut u8 = ptr::null_mut();
        let mut len = 0usize;
        let code = unsafe {
            ffi::ghostty_terminal_selection_format_alloc(self.raw.as_ptr(), ptr::null(), options, &mut out, &mut len)
        };
        if code != ffi::GHOSTTY_SUCCESS {
            return None;
        }
        Some(String::from_utf8_lossy(&unsafe { take_buffer(out, len) }).into_owned())
    }

    pub fn snapshot(&self) -> Result<Vec<u8>, Error> {
        let mut out: *mut u8 = ptr::null_mut();
        let mut len = 0usize;
        check("ghostty_snapshot_encode_alloc", unsafe {
            ffi::ghostty_snapshot_encode_alloc(self.raw.as_ptr(), ptr::null(), &mut out, &mut len)
        })?;
        Ok(unsafe { take_buffer(out, len) })
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        unsafe {
            ffi::ghostty_terminal_free(self.raw.as_ptr());
            drop(Box::from_raw(self.hooks.as_ptr()));
        }
    }
}

unsafe fn take_buffer(ptr: *mut u8, len: usize) -> Vec<u8> {
    if ptr.is_null() {
        return Vec::new();
    }
    let bytes = std::slice::from_raw_parts(ptr, len).to_vec();
    ffi::ghostty_free(ptr::null(), ptr, len);
    bytes
}

unsafe extern "C" fn write_pty(_terminal: ffi::GhosttyTerminal, userdata: *mut c_void, data: *const u8, len: usize) {
    if userdata.is_null() || data.is_null() || len == 0 {
        return;
    }
    let replies = &mut (*(userdata as *mut Hooks)).replies;
    if replies.len().saturating_add(len) <= MAX_PENDING_REPLY_BYTES {
        replies.extend_from_slice(std::slice::from_raw_parts(data, len));
    }
}

unsafe extern "C" fn device_attributes(
    _terminal: ffi::GhosttyTerminal,
    _userdata: *mut c_void,
    out: *mut ffi::GhosttyDeviceAttributes,
) -> bool {
    if out.is_null() {
        return false;
    }
    let mut features = [0u16; 64];
    features[..DA1_FEATURES.len()].copy_from_slice(&DA1_FEATURES);
    *out = ffi::GhosttyDeviceAttributes {
        primary: ffi::GhosttyDeviceAttributesPrimary { conformance_level: 62, features, num_features: DA1_FEATURES.len() },
        secondary: ffi::GhosttyDeviceAttributesSecondary { device_type: 1, firmware_version: 10, rom_cartridge: 0 },
        tertiary: ffi::GhosttyDeviceAttributesTertiary { unit_id: 0 },
    };
    true
}

unsafe extern "C" fn size_report(
    _terminal: ffi::GhosttyTerminal,
    userdata: *mut c_void,
    out: *mut ffi::GhosttySizeReportSize,
) -> bool {
    if userdata.is_null() || out.is_null() {
        return false;
    }
    *out = (*(userdata as *mut Hooks)).size;
    true
}

#[cfg(all(test, kanna_vt_abi))]
mod abi {
    use super::ffi::*;
    use std::mem::{offset_of, size_of};

    extern "C" {
        fn kanna_vt_abi(out: *mut i64, cap: usize) -> usize;
    }

    #[test]
    fn ffi_matches_the_headers() {
        let rust: &[(&str, i64)] = &[
            ("sizeof GhosttyResult", size_of::<GhosttyResult>() as i64),
            ("GHOSTTY_SUCCESS", GHOSTTY_SUCCESS as i64),
            ("GHOSTTY_OUT_OF_MEMORY", GHOSTTY_OUT_OF_MEMORY as i64),
            ("GHOSTTY_INVALID_VALUE", GHOSTTY_INVALID_VALUE as i64),
            ("GHOSTTY_OUT_OF_SPACE", GHOSTTY_OUT_OF_SPACE as i64),
            ("GHOSTTY_NO_VALUE", GHOSTTY_NO_VALUE as i64),
            ("sizeof GhosttyString", size_of::<GhosttyString>() as i64),
            ("GhosttyString.len", offset_of!(GhosttyString, len) as i64),
            ("GHOSTTY_FORMATTER_FORMAT_PLAIN", GHOSTTY_FORMATTER_FORMAT_PLAIN as i64),
            ("GHOSTTY_TERMINAL_OPT_USERDATA", GHOSTTY_TERMINAL_OPT_USERDATA as i64),
            ("GHOSTTY_TERMINAL_OPT_WRITE_PTY", GHOSTTY_TERMINAL_OPT_WRITE_PTY as i64),
            ("GHOSTTY_TERMINAL_OPT_SIZE", GHOSTTY_TERMINAL_OPT_SIZE as i64),
            ("GHOSTTY_TERMINAL_OPT_DEVICE_ATTRIBUTES", GHOSTTY_TERMINAL_OPT_DEVICE_ATTRIBUTES as i64),
            ("GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES", GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES as i64),
            ("GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_LINES", GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_LINES as i64),
            ("GHOSTTY_TERMINAL_OPT_CONTINUATION_MAX_BYTES", GHOSTTY_TERMINAL_OPT_CONTINUATION_MAX_BYTES as i64),
            ("GHOSTTY_TERMINAL_DATA_COLS", GHOSTTY_TERMINAL_DATA_COLS as i64),
            ("GHOSTTY_TERMINAL_DATA_ROWS", GHOSTTY_TERMINAL_DATA_ROWS as i64),
            ("GHOSTTY_TERMINAL_DATA_CURSOR_X", GHOSTTY_TERMINAL_DATA_CURSOR_X as i64),
            ("GHOSTTY_TERMINAL_DATA_CURSOR_Y", GHOSTTY_TERMINAL_DATA_CURSOR_Y as i64),
            ("GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN", GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN as i64),
            ("GHOSTTY_TERMINAL_DATA_CURSOR_VISIBLE", GHOSTTY_TERMINAL_DATA_CURSOR_VISIBLE as i64),
            ("GHOSTTY_TERMINAL_DATA_TITLE", GHOSTTY_TERMINAL_DATA_TITLE as i64),
            ("GHOSTTY_TERMINAL_DATA_PWD", GHOSTTY_TERMINAL_DATA_PWD as i64),
            ("GHOSTTY_TERMINAL_DATA_TOTAL_ROWS", GHOSTTY_TERMINAL_DATA_TOTAL_ROWS as i64),
            ("GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS", GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS as i64),
            ("GHOSTTY_TERMINAL_DATA_SCROLLBACK_MAX_BYTES", GHOSTTY_TERMINAL_DATA_SCROLLBACK_MAX_BYTES as i64),
            ("GHOSTTY_TERMINAL_DATA_SCROLLBACK_MAX_LINES", GHOSTTY_TERMINAL_DATA_SCROLLBACK_MAX_LINES as i64),
            ("sizeof GhosttyTerminalScreen", size_of::<i32>() as i64),
            ("GHOSTTY_TERMINAL_SCREEN_ALTERNATE", GHOSTTY_TERMINAL_SCREEN_ALTERNATE as i64),
            ("GHOSTTY_SNAPSHOT_DECODER_OPT_MAX_CONTINUATION_BYTES", GHOSTTY_SNAPSHOT_DECODER_OPT_MAX_CONTINUATION_BYTES as i64),
            ("GHOSTTY_SNAPSHOT_DECODER_OPT_RETAIN_CONTINUATION", GHOSTTY_SNAPSHOT_DECODER_OPT_RETAIN_CONTINUATION as i64),
            ("GHOSTTY_POINT_TAG_SCREEN", GHOSTTY_POINT_TAG_SCREEN as i64),
            ("sizeof GhosttyPointCoordinate", size_of::<GhosttyPointCoordinate>() as i64),
            ("GhosttyPointCoordinate.y", offset_of!(GhosttyPointCoordinate, y) as i64),
            ("sizeof GhosttyPointValue", size_of::<GhosttyPointValue>() as i64),
            ("sizeof GhosttyPoint", size_of::<GhosttyPoint>() as i64),
            ("GhosttyPoint.value", offset_of!(GhosttyPoint, value) as i64),
            ("sizeof GhosttyGridRef", size_of::<GhosttyGridRef>() as i64),
            ("GhosttyGridRef.node", offset_of!(GhosttyGridRef, node) as i64),
            ("GhosttyGridRef.x", offset_of!(GhosttyGridRef, x) as i64),
            ("GhosttyGridRef.y", offset_of!(GhosttyGridRef, y) as i64),
            ("sizeof GhosttySelection", size_of::<GhosttySelection>() as i64),
            ("GhosttySelection.start", offset_of!(GhosttySelection, start) as i64),
            ("GhosttySelection.end", offset_of!(GhosttySelection, end) as i64),
            ("GhosttySelection.rectangle", offset_of!(GhosttySelection, rectangle) as i64),
            ("sizeof GhosttyTerminalSelectionFormatOptions", size_of::<GhosttyTerminalSelectionFormatOptions>() as i64),
            ("GhosttyTerminalSelectionFormatOptions.emit", offset_of!(GhosttyTerminalSelectionFormatOptions, emit) as i64),
            ("GhosttyTerminalSelectionFormatOptions.unwrap", offset_of!(GhosttyTerminalSelectionFormatOptions, unwrap) as i64),
            ("GhosttyTerminalSelectionFormatOptions.trim", offset_of!(GhosttyTerminalSelectionFormatOptions, trim) as i64),
            ("GhosttyTerminalSelectionFormatOptions.selection", offset_of!(GhosttyTerminalSelectionFormatOptions, selection) as i64),
            ("sizeof GhosttyDeviceAttributesPrimary", size_of::<GhosttyDeviceAttributesPrimary>() as i64),
            ("GhosttyDeviceAttributesPrimary.features", offset_of!(GhosttyDeviceAttributesPrimary, features) as i64),
            ("GhosttyDeviceAttributesPrimary.num_features", offset_of!(GhosttyDeviceAttributesPrimary, num_features) as i64),
            ("sizeof GhosttyDeviceAttributesSecondary", size_of::<GhosttyDeviceAttributesSecondary>() as i64),
            ("GhosttyDeviceAttributesSecondary.firmware_version", offset_of!(GhosttyDeviceAttributesSecondary, firmware_version) as i64),
            ("GhosttyDeviceAttributesSecondary.rom_cartridge", offset_of!(GhosttyDeviceAttributesSecondary, rom_cartridge) as i64),
            ("sizeof GhosttyDeviceAttributesTertiary", size_of::<GhosttyDeviceAttributesTertiary>() as i64),
            ("sizeof GhosttyDeviceAttributes", size_of::<GhosttyDeviceAttributes>() as i64),
            ("GhosttyDeviceAttributes.secondary", offset_of!(GhosttyDeviceAttributes, secondary) as i64),
            ("GhosttyDeviceAttributes.tertiary", offset_of!(GhosttyDeviceAttributes, tertiary) as i64),
            ("sizeof GhosttySizeReportSize", size_of::<GhosttySizeReportSize>() as i64),
            ("GhosttySizeReportSize.columns", offset_of!(GhosttySizeReportSize, columns) as i64),
            ("GhosttySizeReportSize.cell_width", offset_of!(GhosttySizeReportSize, cell_width) as i64),
            ("GhosttySizeReportSize.cell_height", offset_of!(GhosttySizeReportSize, cell_height) as i64),
        ];
        let mut c = vec![0i64; rust.len() + 8];
        let n = unsafe { kanna_vt_abi(c.as_mut_ptr(), c.len()) };
        assert_eq!(n, rust.len(), "csrc/abi.c and this list must name the same items");
        for (i, (name, value)) in rust.iter().enumerate() {
            assert_eq!(*value, c[i], "{name}: Rust has {value}, the C headers {}", c[i]);
        }
    }
}
