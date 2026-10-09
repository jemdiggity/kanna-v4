//! kmux's bridge to merman: one C call that lays out a Mermaid diagram and
//! returns merman's layout JSON. Text is measured by the host through a
//! callback, so the boxes fit the font kmux draws with. See include/kmux_merman.h.

use std::ffi::{c_char, c_void, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

use merman::svg::{
    HostMeasurementResult, HostTextMeasurement, HostTextMeasurementRequest, HostTextMeasurer,
    MeasurementProfileId, TextMeasurementOperation as Op, TextMeasurementPhase,
    TextMeasurementPolicy, TextMeasurementProfileIdentity, TextMetrics, WrapMode,
};
use merman::{OperationControl, RenderOutput, RenderRequest, Renderer, SvgEnvironment, SvgRequest};

#[repr(C)]
pub struct MeasureRequest {
    text: *const c_char,
    text_len: usize,
    font_family: *const c_char,
    font_family_len: usize,
    font_size: f64,
    bold: i32,
    italic: i32,
    max_width: f64,
    html_like: i32,
    kind: i32,
}

#[repr(C)]
#[derive(Default)]
pub struct MeasureResult {
    width: f64,
    height: f64,
    line_count: u32,
    length: f64,
    left: f64,
    right: f64,
    raw_width: f64,
}

type MeasureFn = extern "C" fn(*const MeasureRequest, *mut MeasureResult, *mut c_void) -> i32;

const METRICS: i32 = 0;
const WIDTH: i32 = 1;
const HEIGHT: i32 = 2;
const EXTENTS: i32 = 3;
const WRAPPED_RAW: i32 = 4;

/// The host's measurer. The callback and its context are only used during
/// the synchronous layout call that owns them, on the calling thread.
struct HostMeasurer {
    measure: MeasureFn,
    context: usize,
}

impl HostTextMeasurer for HostMeasurer {
    fn measure(&self, request: HostTextMeasurementRequest<'_>) -> HostMeasurementResult {
        let kind = match request.operation {
            Op::Measure | Op::Wrapped | Op::MermaidCalculateTextDimensions => METRICS,
            Op::WrappedWithRawWidth => WRAPPED_RAW,
            Op::ComputedLength
            | Op::SimpleBBoxWidth
            | Op::RawBBoxWidth
            | Op::TspanBBoxWidth
            | Op::WrapProbeBBoxWidth
            | Op::BoundingClientRectWidth
            | Op::CanvasMeasureTextWidth => WIDTH,
            Op::TspanBBoxHeight | Op::SimpleBBoxHeight | Op::RawBBoxHeight => HEIGHT,
            Op::BBoxX | Op::BBoxXWithAsciiOverhang | Op::TitleBBoxX => EXTENTS,
            // Baseline offsets only place SVG text; kmux draws its own.
            Op::CreateTextBBoxYOffset | Op::CreateTextMiddleBBoxYOffset => return Ok(None),
        };
        let style = request.style;
        let family = style.font_family.as_deref().unwrap_or("");
        let weight = style.font_weight.as_deref().unwrap_or("");
        let bold = weight == "bold" || weight == "bolder" || weight.parse::<u32>().is_ok_and(|w| w >= 600);
        let c_request = MeasureRequest {
            text: request.text.as_ptr().cast(),
            text_len: request.text.len(),
            font_family: family.as_ptr().cast(),
            font_family_len: family.len(),
            font_size: style.font_size,
            bold: bold as i32,
            italic: matches!(style.font_style.as_deref(), Some("italic" | "oblique")) as i32,
            max_width: request.max_width.unwrap_or(-1.0),
            html_like: matches!(request.wrap_mode, WrapMode::HtmlLike) as i32,
            kind,
        };
        let mut out = MeasureResult::default();
        if (self.measure)(&c_request, &mut out, self.context as *mut c_void) == 0 {
            return Ok(None);
        }
        let metrics = TextMetrics { width: out.width, height: out.height, line_count: out.line_count as usize };
        Ok(Some(match kind {
            METRICS => HostTextMeasurement::Metrics(metrics),
            WRAPPED_RAW => HostTextMeasurement::WrappedWithRawWidth { metrics, raw_width: Some(out.raw_width) },
            EXTENTS => HostTextMeasurement::HorizontalExtents { left: out.left, right: out.right },
            _ => HostTextMeasurement::Length(out.length),
        }))
    }
}

fn layout(source: &str, measure: Option<MeasureFn>, context: *mut c_void) -> Result<serde_json::Value, String> {
    let mut environment = SvgEnvironment::deterministic();
    if let Some(measure) = measure {
        let identity = TextMeasurementProfileIdentity::new(
            MeasurementProfileId::new("kmux.coretext").map_err(|e| format!("{e:?}"))?,
            "1",
        )
        .map_err(|e| format!("{e:?}"))?;
        let host = Arc::new(HostMeasurer { measure, context: context as usize });
        environment = environment.with_text_measurement_policy(TextMeasurementPolicy::host_display(
            identity,
            host,
            TextMeasurementPhase::ALL,
        ));
    }
    let request = SvgRequest { environment, ..Default::default() };
    let output = Renderer::new()
        .render(RenderRequest::layout_json(source, OperationControl::new(), request))
        .map_err(|e| e.to_string())?;
    match output {
        RenderOutput::LayoutJson(Some(layout)) => Ok(layout.layout().clone()),
        _ => Err("No diagram found".into()),
    }
}

/// See include/kmux_merman.h.
///
/// # Safety
/// `source` must point to `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn kmux_merman_layout(
    source: *const c_char,
    len: usize,
    measure: Option<MeasureFn>,
    context: *mut c_void,
) -> *mut c_char {
    let bytes = if source.is_null() { &[][..] } else { std::slice::from_raw_parts(source.cast::<u8>(), len) };
    let json = match std::str::from_utf8(bytes) {
        Err(_) => serde_json::json!({ "error": "The diagram is not valid UTF-8" }),
        Ok(text) => match catch_unwind(AssertUnwindSafe(|| layout(text, measure, context))) {
            Ok(Ok(value)) => value,
            Ok(Err(message)) => serde_json::json!({ "error": message }),
            Err(_) => serde_json::json!({ "error": "merman crashed on this diagram" }),
        },
    };
    CString::new(json.to_string()).map(CString::into_raw).unwrap_or(std::ptr::null_mut())
}

/// Frees a string from kmux_merman_layout.
///
/// # Safety
/// `json` must come from kmux_merman_layout and not be freed twice.
#[no_mangle]
pub unsafe extern "C" fn kmux_merman_free(json: *mut c_char) {
    if !json.is_null() {
        drop(CString::from_raw(json));
    }
}
