use kanna_mux::json::{encode, json, Value};
use kanna_mux::{LayoutOwner, StateFile};
use std::ffi::{c_char, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr::null_mut;

pub struct KannaLayout(LayoutOwner);

fn text(pointer: *const c_char) -> Option<String> {
    (!pointer.is_null()).then(|| unsafe { CStr::from_ptr(pointer) }.to_string_lossy().into_owned())
}

fn give(text: String) -> *mut c_char {
    CString::new(text.replace('\0', "")).map_or(null_mut(), CString::into_raw)
}

#[no_mangle]
pub extern "C" fn kanna_layout_open(state_path: *const c_char, fresh: bool, problem: *mut *mut c_char) -> *mut KannaLayout {
    let file = text(state_path).filter(|path| !path.is_empty()).map(|path| StateFile::new(&path));
    let opened = catch_unwind(|| LayoutOwner::open(file, fresh));
    let (owner, trouble) = opened.unwrap_or_else(|_| (LayoutOwner::new(), Some("reading the saved layout failed".into())));
    if !problem.is_null() {
        unsafe { *problem = trouble.map_or(null_mut(), give) };
    }
    Box::into_raw(Box::new(KannaLayout(owner)))
}

#[no_mangle]
pub extern "C" fn kanna_layout_call(layout: *mut KannaLayout, request: *const c_char) -> *mut c_char {
    let Some(layout) = (unsafe { layout.as_mut() }) else { return give(json!({ "ok": false, "error": { "code": "bad_request", "message": "no layout" } }).to_string()) };
    let request: Value = text(request).and_then(|r| serde_json::from_str(&r).ok()).unwrap_or(Value::Null);
    let reply = catch_unwind(AssertUnwindSafe(|| layout.0.handle(&request)))
        .unwrap_or_else(|_| json!({ "ok": false, "error": { "code": "internal", "message": "the layout failed" } }));
    give(encode(&reply))
}

#[no_mangle]
pub extern "C" fn kanna_layout_fraction(value: f64) -> *mut c_char {
    give(kanna_mux::fraction::format(value))
}

#[no_mangle]
pub extern "C" fn kanna_layout_free_string(string: *mut c_char) {
    if !string.is_null() {
        drop(unsafe { CString::from_raw(string) });
    }
}

#[no_mangle]
pub extern "C" fn kanna_layout_close(layout: *mut KannaLayout) {
    if !layout.is_null() {
        drop(unsafe { Box::from_raw(layout) });
    }
}
