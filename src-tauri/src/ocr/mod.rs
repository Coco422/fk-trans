use image::{DynamicImage, ImageFormat, RgbaImage};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Mutex;
use std::time::Instant;
use tauri::PhysicalPosition;
use uuid::Uuid;

const MIN_SELECTION_CSS_PX: f64 = 8.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrSelectionRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrSelectionPayload {
    pub session_id: String,
    pub image_data_url: String,
    pub monitor_x: i32,
    pub monitor_y: i32,
    pub monitor_width: u32,
    pub monitor_height: u32,
    pub image_width: u32,
    pub image_height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrDiagnostic {
    pub enabled: bool,
    pub backend: String,
    pub ready: bool,
    pub reason: Option<String>,
    pub screen_capture_ready: bool,
    pub last_result: Option<String>,
    pub last_error: Option<String>,
    pub last_elapsed_ms: Option<u64>,
}

pub struct OcrCrop {
    pub png_bytes: Vec<u8>,
    pub popup_anchor: PhysicalPosition<f64>,
}

struct OcrSession {
    payload: OcrSelectionPayload,
    image: RgbaImage,
}

#[derive(Default)]
struct OcrRuntimeState {
    sessions: HashMap<String, OcrSession>,
    latest_payload: Option<OcrSelectionPayload>,
    last_result: Option<String>,
    last_error: Option<String>,
    last_elapsed_ms: Option<u64>,
    screen_capture_ready: bool,
}

pub struct OcrRuntime {
    state: Mutex<OcrRuntimeState>,
}

impl OcrRuntime {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(OcrRuntimeState {
                screen_capture_ready: cfg!(target_os = "macos"),
                ..OcrRuntimeState::default()
            }),
        }
    }

    pub fn snapshot(&self, enabled: bool) -> OcrDiagnostic {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let platform_ready = cfg!(target_os = "macos");
        OcrDiagnostic {
            enabled,
            backend: if platform_ready {
                "apple_vision".to_string()
            } else {
                "unsupported".to_string()
            },
            ready: enabled && platform_ready,
            reason: if platform_ready {
                None
            } else {
                Some("OCR is only implemented on macOS in this version".to_string())
            },
            screen_capture_ready: state.screen_capture_ready,
            last_result: state.last_result.clone(),
            last_error: state.last_error.clone(),
            last_elapsed_ms: state.last_elapsed_ms,
        }
    }

    pub fn latest_payload(&self) -> Option<OcrSelectionPayload> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .latest_payload
            .clone()
    }

    pub fn start_selection_session(
        &self,
        cursor: PhysicalPosition<f64>,
    ) -> Result<OcrSelectionPayload, String> {
        let started = Instant::now();
        let captured = match capture_current_monitor(cursor) {
            Ok(captured) => captured,
            Err(error) => {
                let mut state = self
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                state.screen_capture_ready = false;
                state.last_error = Some(error.clone());
                return Err(error);
            }
        };
        let image_data_url = match png_data_url(&captured.image) {
            Ok(data_url) => data_url,
            Err(error) => {
                let mut state = self
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                state.last_error = Some(error.clone());
                return Err(error);
            }
        };
        let session_id = Uuid::new_v4().to_string();
        let payload = OcrSelectionPayload {
            session_id: session_id.clone(),
            image_data_url,
            monitor_x: captured.monitor_x,
            monitor_y: captured.monitor_y,
            monitor_width: captured.monitor_width,
            monitor_height: captured.monitor_height,
            image_width: captured.image.width(),
            image_height: captured.image.height(),
        };

        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.sessions.insert(
            session_id,
            OcrSession {
                payload: payload.clone(),
                image: captured.image,
            },
        );
        state.latest_payload = Some(payload.clone());
        state.screen_capture_ready = true;
        state.last_result = Some(format!(
            "Selection session ready in {} ms",
            started.elapsed().as_millis()
        ));
        state.last_error = None;
        state.last_elapsed_ms = Some(started.elapsed().as_millis() as u64);

        Ok(payload)
    }

    pub fn crop_selection(
        &self,
        session_id: &str,
        selection: OcrSelectionRect,
    ) -> Result<Option<OcrCrop>, String> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(session) = state.sessions.remove(session_id) else {
            state.last_error = Some("OCR selection session was not found".to_string());
            return Err("OCR selection session was not found".to_string());
        };

        let Some(rect) = map_selection_to_image(
            &selection,
            session.payload.monitor_width as f64,
            session.payload.monitor_height as f64,
            session.payload.image_width,
            session.payload.image_height,
        ) else {
            state.last_result = Some("OCR selection canceled: selected area too small".to_string());
            state.last_error = None;
            return Ok(None);
        };

        let cropped =
            image::imageops::crop_imm(&session.image, rect.x, rect.y, rect.width, rect.height)
                .to_image();
        let png_bytes = encode_png(&cropped)?;
        let anchor_x = selection
            .x
            .max(selection.x + selection.width)
            .clamp(0.0, session.payload.monitor_width as f64);
        let anchor_y = selection
            .y
            .max(selection.y + selection.height)
            .clamp(0.0, session.payload.monitor_height as f64);
        let popup_anchor = PhysicalPosition::new(
            session.payload.monitor_x as f64 + anchor_x,
            session.payload.monitor_y as f64 + anchor_y,
        );
        state.last_result = Some(format!(
            "OCR selection captured: {}x{} px",
            rect.width, rect.height
        ));
        state.last_error = None;

        Ok(Some(OcrCrop {
            png_bytes,
            popup_anchor,
        }))
    }

    pub fn cancel_selection(&self, session_id: &str) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.sessions.remove(session_id);
        state.last_result = Some("OCR selection canceled".to_string());
        state.last_error = None;
    }

    pub fn mark_result(&self, result: impl Into<String>, elapsed_ms: Option<u64>) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.last_result = Some(result.into());
        state.last_error = None;
        if let Some(elapsed_ms) = elapsed_ms {
            state.last_elapsed_ms = Some(elapsed_ms);
        }
    }

    pub fn mark_error(&self, error: impl Into<String>) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.last_result = None;
        state.last_error = Some(error.into());
    }

    pub fn mark_screen_capture_error(&self, error: impl Into<String>) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.screen_capture_ready = false;
        state.last_result = None;
        state.last_error = Some(error.into());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageSelectionRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

pub fn map_selection_to_image(
    selection: &OcrSelectionRect,
    display_width: f64,
    display_height: f64,
    image_width: u32,
    image_height: u32,
) -> Option<ImageSelectionRect> {
    if display_width <= 0.0 || display_height <= 0.0 || image_width == 0 || image_height == 0 {
        return None;
    }

    let x1 = selection.x.min(selection.x + selection.width);
    let x2 = selection.x.max(selection.x + selection.width);
    let y1 = selection.y.min(selection.y + selection.height);
    let y2 = selection.y.max(selection.y + selection.height);
    let x1 = x1.clamp(0.0, display_width);
    let x2 = x2.clamp(0.0, display_width);
    let y1 = y1.clamp(0.0, display_height);
    let y2 = y2.clamp(0.0, display_height);

    if x2 - x1 < MIN_SELECTION_CSS_PX || y2 - y1 < MIN_SELECTION_CSS_PX {
        return None;
    }

    let scale_x = image_width as f64 / display_width;
    let scale_y = image_height as f64 / display_height;
    let px1 = (x1 * scale_x).floor().clamp(0.0, image_width as f64) as u32;
    let py1 = (y1 * scale_y).floor().clamp(0.0, image_height as f64) as u32;
    let px2 = (x2 * scale_x).ceil().clamp(0.0, image_width as f64) as u32;
    let py2 = (y2 * scale_y).ceil().clamp(0.0, image_height as f64) as u32;

    if px2 <= px1 || py2 <= py1 {
        return None;
    }

    Some(ImageSelectionRect {
        x: px1,
        y: py1,
        width: px2 - px1,
        height: py2 - py1,
    })
}

fn png_data_url(image: &RgbaImage) -> Result<String, String> {
    use base64::Engine;

    let bytes = encode_png(image)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(format!("data:image/png;base64,{}", encoded))
}

fn encode_png(image: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut cursor = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image.clone())
        .write_to(&mut cursor, ImageFormat::Png)
        .map_err(|e| format!("Failed to encode screenshot PNG: {}", e))?;
    Ok(cursor.into_inner())
}

struct CapturedMonitor {
    image: RgbaImage,
    monitor_x: i32,
    monitor_y: i32,
    monitor_width: u32,
    monitor_height: u32,
}

#[cfg(target_os = "macos")]
fn capture_current_monitor(cursor: PhysicalPosition<f64>) -> Result<CapturedMonitor, String> {
    use xcap::Monitor;

    let monitor = Monitor::from_point(cursor.x.round() as i32, cursor.y.round() as i32)
        .or_else(|_| {
            Monitor::all()?
                .into_iter()
                .next()
                .ok_or_else(|| xcap::XCapError::new("No monitor found"))
        })
        .map_err(|e| format!("Failed to locate monitor for OCR: {}", e))?;
    let monitor_x = monitor
        .x()
        .map_err(|e| format!("Failed to read monitor x: {}", e))?;
    let monitor_y = monitor
        .y()
        .map_err(|e| format!("Failed to read monitor y: {}", e))?;
    let monitor_width = monitor
        .width()
        .map_err(|e| format!("Failed to read monitor width: {}", e))?;
    let monitor_height = monitor
        .height()
        .map_err(|e| format!("Failed to read monitor height: {}", e))?;
    let image = monitor
        .capture_image()
        .map_err(|e| format!("Failed to capture screen for OCR: {}", e))?;

    Ok(CapturedMonitor {
        image,
        monitor_x,
        monitor_y,
        monitor_width,
        monitor_height,
    })
}

#[cfg(not(target_os = "macos"))]
fn capture_current_monitor(_cursor: PhysicalPosition<f64>) -> Result<CapturedMonitor, String> {
    Err("OCR screen capture is only implemented on macOS".to_string())
}

#[cfg(target_os = "macos")]
pub fn recognize_text_from_png(png: &[u8]) -> Result<String, String> {
    macos_vision::recognize_text_from_png(png)
}

#[cfg(not(target_os = "macos"))]
pub fn recognize_text_from_png(_png: &[u8]) -> Result<String, String> {
    Err("Apple Vision OCR is only available on macOS".to_string())
}

#[cfg(target_os = "macos")]
mod macos_vision {
    use objc::runtime::{Object, BOOL, NO, YES};
    use objc::{class, msg_send, sel, sel_impl};
    use std::ffi::CStr;
    use std::ptr;

    #[link(name = "Vision", kind = "framework")]
    extern "C" {}

    #[link(name = "Foundation", kind = "framework")]
    extern "C" {}

    pub fn recognize_text_from_png(png: &[u8]) -> Result<String, String> {
        unsafe {
            let pool: *mut Object = msg_send![class!(NSAutoreleasePool), new];
            let result = recognize_text_from_png_inner(png);
            let _: () = msg_send![pool, drain];
            result
        }
    }

    unsafe fn recognize_text_from_png_inner(png: &[u8]) -> Result<String, String> {
        let data: *mut Object = msg_send![
            class!(NSData),
            dataWithBytes: png.as_ptr()
            length: png.len()
        ];
        if data.is_null() {
            return Err("Failed to create NSData for OCR image".to_string());
        }

        let request: *mut Object = msg_send![class!(VNRecognizeTextRequest), new];
        if request.is_null() {
            return Err("Failed to create Apple Vision text request".to_string());
        }
        let _: () = msg_send![request, setRecognitionLevel: 0i64];
        let _: () = msg_send![request, setUsesLanguageCorrection: YES];

        let options: *mut Object = msg_send![class!(NSDictionary), dictionary];
        let handler: *mut Object = msg_send![class!(VNImageRequestHandler), alloc];
        let handler: *mut Object = msg_send![handler, initWithData: data options: options];
        if handler.is_null() {
            let _: () = msg_send![request, release];
            return Err("Failed to create Apple Vision image request handler".to_string());
        }

        let requests: *mut Object = msg_send![class!(NSArray), arrayWithObject: request];
        let mut error: *mut Object = ptr::null_mut();
        let ok: BOOL = msg_send![handler, performRequests: requests error: &mut error];
        if ok == NO {
            let reason = ns_error_description(error)
                .unwrap_or_else(|| "Apple Vision OCR request failed".to_string());
            let _: () = msg_send![handler, release];
            let _: () = msg_send![request, release];
            return Err(reason);
        }

        let observations: *mut Object = msg_send![request, results];
        let mut lines = Vec::new();
        if !observations.is_null() {
            let count: usize = msg_send![observations, count];
            for index in 0..count {
                let observation: *mut Object = msg_send![observations, objectAtIndex: index];
                let candidates: *mut Object = msg_send![observation, topCandidates: 1usize];
                if candidates.is_null() {
                    continue;
                }
                let candidate_count: usize = msg_send![candidates, count];
                if candidate_count == 0 {
                    continue;
                }
                let candidate: *mut Object = msg_send![candidates, objectAtIndex: 0usize];
                let string: *mut Object = msg_send![candidate, string];
                if let Some(text) = ns_string_to_string(string) {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        lines.push(trimmed.to_string());
                    }
                }
            }
        }

        let _: () = msg_send![handler, release];
        let _: () = msg_send![request, release];

        let text = lines.join("\n").trim().to_string();
        if text.is_empty() {
            Err("OCR found no readable text".to_string())
        } else {
            Ok(text)
        }
    }

    unsafe fn ns_error_description(error: *mut Object) -> Option<String> {
        if error.is_null() {
            return None;
        }
        let description: *mut Object = msg_send![error, localizedDescription];
        ns_string_to_string(description)
    }

    unsafe fn ns_string_to_string(ns_string: *mut Object) -> Option<String> {
        if ns_string.is_null() {
            return None;
        }
        let c_string: *const std::os::raw::c_char = msg_send![ns_string, UTF8String];
        if c_string.is_null() {
            return None;
        }
        Some(CStr::from_ptr(c_string).to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiny_selection_is_canceled() {
        let rect = map_selection_to_image(
            &OcrSelectionRect {
                x: 10.0,
                y: 10.0,
                width: 7.0,
                height: 20.0,
            },
            100.0,
            100.0,
            200,
            200,
        );

        assert_eq!(rect, None);
    }

    #[test]
    fn selection_maps_and_clamps_to_image_pixels() {
        let rect = map_selection_to_image(
            &OcrSelectionRect {
                x: -10.0,
                y: 10.0,
                width: 60.0,
                height: 30.0,
            },
            100.0,
            100.0,
            200,
            300,
        )
        .unwrap();

        assert_eq!(
            rect,
            ImageSelectionRect {
                x: 0,
                y: 30,
                width: 100,
                height: 90
            }
        );
    }
}
