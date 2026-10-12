use crate::{highlight, language_for, Scope, Span, CAPTURES, LANGUAGES};
use std::ffi::{c_char, CStr, CString};
use std::panic::catch_unwind;
use std::ptr::null;
use std::sync::OnceLock;

#[repr(C)]
pub struct KannaSyntaxSpan {
    pub start: u32,
    pub end: u32,
    pub capture: u32,
}

#[repr(C)]
pub struct KannaSyntaxScope {
    pub first: u32,
    pub last: u32,
}

#[repr(C)]
pub struct KannaSyntaxResult {
    pub spans: *mut KannaSyntaxSpan,
    pub span_count: usize,
    pub scopes: *mut KannaSyntaxScope,
    pub scope_count: usize,
}

fn names(list: impl Iterator<Item = &'static str>) -> Vec<CString> {
    list.map(|name| CString::new(name).unwrap()).collect()
}

fn capture_names() -> &'static [CString] {
    static NAMES: OnceLock<Vec<CString>> = OnceLock::new();
    NAMES.get_or_init(|| names(CAPTURES.iter().copied()))
}

fn language_names() -> &'static [CString] {
    static NAMES: OnceLock<Vec<CString>> = OnceLock::new();
    NAMES.get_or_init(|| names(LANGUAGES.iter().map(|l| l.name)))
}

fn text<'a>(pointer: *const c_char) -> &'a str {
    if pointer.is_null() {
        return "";
    }
    unsafe { CStr::from_ptr(pointer) }.to_str().unwrap_or("")
}

#[no_mangle]
pub extern "C" fn kanna_syntax_language(file_name: *const c_char, first_line: *const c_char) -> *const c_char {
    let found = language_for(text(file_name), text(first_line));
    found.and_then(|name| LANGUAGES.iter().position(|l| l.name == name)).map_or(null(), |i| language_names()[i].as_ptr())
}

#[no_mangle]
pub extern "C" fn kanna_syntax_capture_count() -> usize {
    CAPTURES.len()
}

#[no_mangle]
pub extern "C" fn kanna_syntax_capture_name(index: usize) -> *const c_char {
    capture_names().get(index).map_or(null(), |name| name.as_ptr())
}

#[no_mangle]
pub extern "C" fn kanna_syntax_highlight(language: *const c_char, bytes: *const u8, length: usize) -> *mut KannaSyntaxResult {
    let language = text(language).to_string();
    let source = if bytes.is_null() { &[][..] } else { unsafe { std::slice::from_raw_parts(bytes, length) } };
    let source = String::from_utf8_lossy(source).into_owned();
    let found = catch_unwind(|| highlight(&language, &source)).ok().flatten().unwrap_or_default();
    let spans: Box<[KannaSyntaxSpan]> = found.spans.iter().map(|&Span { start, end, capture }| KannaSyntaxSpan { start, end, capture }).collect();
    let scopes: Box<[KannaSyntaxScope]> = found.scopes.iter().map(|&Scope { first, last }| KannaSyntaxScope { first, last }).collect();
    let (span_count, scope_count) = (spans.len(), scopes.len());
    Box::into_raw(Box::new(KannaSyntaxResult {
        spans: Box::into_raw(spans) as *mut KannaSyntaxSpan,
        span_count,
        scopes: Box::into_raw(scopes) as *mut KannaSyntaxScope,
        scope_count,
    }))
}

#[no_mangle]
pub extern "C" fn kanna_syntax_free(result: *mut KannaSyntaxResult) {
    if result.is_null() {
        return;
    }
    let result = unsafe { Box::from_raw(result) };
    unsafe {
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(result.spans, result.span_count)));
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(result.scopes, result.scope_count)));
    }
}
