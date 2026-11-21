#![expect(
    deprecated,
    reason = "ADR-0024 requires projectM's OpenGL renderer for authentic MilkDrop compatibility"
)]
use std::ffi::{CStr, CString, c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::NonNull;
use std::sync::OnceLock;

use cocoa::appkit::{
    NSOpenGLContext, NSOpenGLPFAOpenGLProfiles, NSOpenGLPixelFormat, NSOpenGLPixelFormatAttribute,
    NSOpenGLView, NSView,
};
use cocoa::base::{BOOL, NO, id, nil};
use cocoa::foundation::{NSPoint, NSRect, NSSize};
use gpui_kit::{Bounds, Pixels, Window};
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel};
use objc::{class, msg_send, sel, sel_impl};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::{
    MilkdropError, MilkdropFailureMode, MilkdropPcmConsumer, PresetSwapError,
    install_candidate_with_rollback,
};

const TARGET_FPS: i32 = 60;

pub(super) struct ProjectmView {
    view: id,
    context: id,
    projectm: NonNull<projectm_sys::projectm>,
    preset_error: Box<PresetErrorState>,
    pcm_scratch: Box<[f32]>,
    drawable_size: (usize, usize),
    active_preset: String,
}

impl ProjectmView {
    pub(super) fn attach(
        window: &mut Window,
        bounds: Bounds<Pixels>,
        preset: &str,
        failure_injection: Option<MilkdropFailureMode>,
    ) -> Result<Self, MilkdropError> {
        let inject_preset_failure = match failure_injection {
            Some(MilkdropFailureMode::Preset) => true,
            #[cfg(feature = "milkdrop-test-hooks")]
            Some(failure) => return Err(failure.into_error()),
            None => false,
        };
        if !is_main_thread() {
            return Err(MilkdropError::backend(
                "native OpenGL view must be created on the macOS main thread",
            ));
        }
        let host_view = appkit_host_view(window)?;
        // SAFETY: the raw handle guarantees a live NSView on the main thread.
        let host_height = unsafe { NSView::bounds(host_view) }.size.height;
        let frame = frame_from_gpui_bounds(bounds, host_height);
        let attributes = [
            NSOpenGLPixelFormatAttribute::NSOpenGLPFAOpenGLProfile as u32,
            NSOpenGLPFAOpenGLProfiles::NSOpenGLProfileVersion3_2Core as u32,
            NSOpenGLPixelFormatAttribute::NSOpenGLPFAAccelerated as u32,
            NSOpenGLPixelFormatAttribute::NSOpenGLPFADoubleBuffer as u32,
            0,
        ];

        // SAFETY: AppKit objects are created and messaged on the main thread. The attribute list is
        // zero-terminated and the returned owned objects are checked for nil before use.
        let (view, context) = unsafe {
            let pixel_format = NSOpenGLPixelFormat::alloc(nil).initWithAttributes_(&attributes);
            if pixel_format == nil {
                return Err(MilkdropError::backend(
                    "AppKit could not create an accelerated OpenGL 3.2 pixel format",
                ));
            }
            let allocated: id = msg_send![milkdrop_view_class(), alloc];
            let view = NSOpenGLView::initWithFrame_pixelFormat_(allocated, frame, pixel_format);
            let _: () = msg_send![pixel_format, release];
            if view == nil {
                return Err(MilkdropError::backend(
                    "AppKit could not create the MilkDrop OpenGL child view",
                ));
            }
            NSView::setWantsBestResolutionOpenGLSurface_(view, cocoa::base::YES);
            let _: () = msg_send![view, setAccessibilityElement: NO];
            host_view.addSubview_(view);
            let context: id = msg_send![view, openGLContext];
            if context == nil {
                NSView::removeFromSuperview(view);
                let _: () = msg_send![view, release];
                return Err(MilkdropError::backend(
                    "MilkDrop OpenGL child view has no context",
                ));
            }
            NSOpenGLContext::makeCurrentContext(context);
            (view, context)
        };

        let projectm = {
            // SAFETY: the child view's OpenGL context is current and projectM returns either a
            // valid uniquely owned handle or null.
            let handle = unsafe { projectm_sys::projectm_create() };
            let Some(handle) = NonNull::new(handle) else {
                // SAFETY: the view is retained by this function and currently attached.
                unsafe {
                    NSView::removeFromSuperview(view);
                    let _: () = msg_send![view, release];
                }
                return Err(MilkdropError::backend(
                    "projectM could not initialize in the native OpenGL view",
                ));
            };
            handle
        };
        let max_samples = {
            // SAFETY: this reads a projectM process constant and has no pointer inputs.
            unsafe { projectm_sys::projectm_pcm_get_max_samples() as usize }
        }
        .max(1);
        let mut result = Self {
            view,
            context,
            projectm,
            preset_error: Box::new(PresetErrorState::default()),
            pcm_scratch: vec![0.0; max_samples].into_boxed_slice(),
            drawable_size: (0, 0),
            active_preset: String::new(),
        };
        if inject_preset_failure {
            return Err(MilkdropFailureMode::Preset.into_error());
        }
        result.load_preset_data(preset).map_err(|error| {
            MilkdropError::backend(format!("original AMP808 preset could not load: {error}"))
        })?;
        result.active_preset = preset.to_owned();
        result.resize(bounds)?;
        result.show();
        Ok(result)
    }

    pub(super) fn present(
        &mut self,
        bounds: Bounds<Pixels>,
        pcm: &mut MilkdropPcmConsumer,
    ) -> Result<(), MilkdropError> {
        self.make_current();
        self.resize(bounds)?;
        loop {
            let count = pcm.pop_into(&mut self.pcm_scratch);
            if count == 0 {
                break;
            }
            let count = u32::try_from(count)
                .map_err(|_| MilkdropError::backend("PCM block exceeds projectM limits"))?;
            // SAFETY: projectM is live, the scratch buffer contains `count` mono samples, and the
            // synchronous function does not retain the pointer.
            unsafe {
                projectm_sys::projectm_pcm_add_float(
                    self.projectm.as_ptr(),
                    self.pcm_scratch.as_ptr(),
                    count,
                    projectm_sys::projectm_channels_PROJECTM_MONO,
                );
            }
        }

        // SAFETY: projectM owns all renderer resources and the native view's context is current.
        unsafe { projectm_sys::projectm_opengl_render_frame(self.projectm.as_ptr()) };
        // SAFETY: the context is a live NSOpenGLContext owned by the retained NSOpenGLView.
        unsafe { NSOpenGLContext::flushBuffer(self.context) };
        if let Some(error) = self.preset_error.message.take() {
            return Err(MilkdropError::backend(format!(
                "active MilkDrop preset could not render: {error}"
            )));
        }
        self.show();
        Ok(())
    }

    pub(super) fn hide(&mut self) {
        // SAFETY: the retained view is live and all calls occur on AppKit's main thread.
        unsafe {
            let _: () = msg_send![self.view, setHidden: cocoa::base::YES];
        }
    }

    pub(super) fn align(&mut self, bounds: Bounds<Pixels>) -> Result<(), MilkdropError> {
        self.make_current();
        self.resize(bounds)?;
        self.show();
        Ok(())
    }

    fn show(&mut self) {
        // SAFETY: the retained view is live and all calls occur on AppKit's main thread.
        unsafe {
            let _: () = msg_send![self.view, setHidden: NO];
        }
    }

    pub(super) fn install_preset(&mut self, preset: &str) -> Result<(), PresetSwapError> {
        let mut active = std::mem::take(&mut self.active_preset);
        let result = install_candidate_with_rollback(&mut active, preset, |source| {
            self.load_preset_data(source)
        });
        self.active_preset = active;
        result
    }

    fn load_preset_data(&mut self, preset: &str) -> Result<(), String> {
        let preset = CString::new(preset)
            .map_err(|_| "MilkDrop preset contains an interior nul byte".to_owned())?;
        let error_state: *mut PresetErrorState = &mut *self.preset_error;
        // SAFETY: projectM is live; `error_state` points to a pinned Box owned by this view for the
        // callback lifetime; the preset CString outlives the synchronous load call.
        unsafe {
            projectm_sys::projectm_set_preset_switch_failed_event_callback(
                self.projectm.as_ptr(),
                Some(preset_switch_failed),
                error_state.cast(),
            );
            projectm_sys::projectm_set_fps(self.projectm.as_ptr(), TARGET_FPS);
            projectm_sys::projectm_load_preset_data(self.projectm.as_ptr(), preset.as_ptr(), false);
        }
        if let Some(error) = self.preset_error.message.take() {
            return Err(error);
        }
        Ok(())
    }

    fn resize(&mut self, bounds: Bounds<Pixels>) -> Result<(), MilkdropError> {
        // SAFETY: the retained view is attached to a live GPUI NSView on the main thread.
        let superview = unsafe { NSView::superview(self.view) };
        if superview == nil {
            return Err(MilkdropError::backend(
                "MilkDrop child view is no longer attached to the GPUI window",
            ));
        }
        // SAFETY: both views are live AppKit objects on the main thread.
        let backing = unsafe {
            let parent_height = NSView::bounds(superview).size.height;
            let frame = frame_from_gpui_bounds(bounds, parent_height);
            NSView::setFrameSize(self.view, frame.size);
            NSView::setFrameOrigin(self.view, frame.origin);
            NSOpenGLContext::update(self.context);
            NSView::convertRectToBacking(self.view, NSView::bounds(self.view))
        };
        let width = drawable_dimension(backing.size.width, "width")?;
        let height = drawable_dimension(backing.size.height, "height")?;
        if self.drawable_size != (width, height) {
            // SAFETY: projectM is live and the drawable dimensions are validated non-zero values.
            unsafe {
                projectm_sys::projectm_set_window_size(self.projectm.as_ptr(), width, height)
            };
            self.drawable_size = (width, height);
        }
        Ok(())
    }

    fn make_current(&self) {
        // SAFETY: the context is live and main-thread confined with its owning view.
        unsafe { NSOpenGLContext::makeCurrentContext(self.context) };
    }
}

impl Drop for ProjectmView {
    fn drop(&mut self) {
        self.make_current();
        // SAFETY: this adapter uniquely owns projectM and the retained child view. The callback is
        // removed before its user-data Box drops, and all teardown occurs on the main thread.
        unsafe {
            projectm_sys::projectm_set_preset_switch_failed_event_callback(
                self.projectm.as_ptr(),
                None,
                std::ptr::null_mut(),
            );
            projectm_sys::projectm_destroy(self.projectm.as_ptr());
            NSOpenGLContext::clearDrawable(self.context);
            NSView::removeFromSuperview(self.view);
            let _: () = msg_send![self.view, release];
            NSOpenGLContext::clearCurrentContext(nil);
        }
    }
}

#[derive(Default)]
struct PresetErrorState {
    message: Option<String>,
}

extern "C" fn preset_switch_failed(
    _preset_filename: *const c_char,
    message: *const c_char,
    user_data: *mut c_void,
) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if user_data.is_null() {
            return;
        }
        let message = if message.is_null() {
            "unknown projectM preset error".to_owned()
        } else {
            // SAFETY: projectM guarantees this pointer is a valid nul-terminated string for the
            // duration of the callback; it is copied before returning.
            unsafe { CStr::from_ptr(message) }
                .to_string_lossy()
                .into_owned()
        };
        // SAFETY: registration passes the stable address of this view's live PresetErrorState Box
        // and unregisters before the Box is dropped.
        unsafe { &mut *user_data.cast::<PresetErrorState>() }.message = Some(message);
    }));
}

fn appkit_host_view(window: &mut Window) -> Result<id, MilkdropError> {
    let handle = window.window_handle().map_err(|error| {
        MilkdropError::backend(format!("GPUI AppKit handle unavailable: {error}"))
    })?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return Err(MilkdropError::backend(
            "GPUI did not provide an AppKit window handle",
        ));
    };
    Ok(handle.ns_view.as_ptr().cast())
}

fn frame_from_gpui_bounds(bounds: Bounds<Pixels>, parent_height: f64) -> NSRect {
    appkit_frame(
        f64::from(bounds.origin.x.as_f32()),
        f64::from(bounds.origin.y.as_f32()),
        f64::from(bounds.size.width.as_f32()),
        f64::from(bounds.size.height.as_f32()),
        parent_height,
    )
}

fn appkit_frame(x: f64, y: f64, width: f64, height: f64, parent_height: f64) -> NSRect {
    NSRect::new(
        NSPoint::new(x, parent_height - y - height),
        NSSize::new(width, height),
    )
}

fn drawable_dimension(value: f64, name: &str) -> Result<usize, MilkdropError> {
    if !value.is_finite() || value < 1.0 || value > usize::MAX as f64 {
        return Err(MilkdropError::backend(format!(
            "MilkDrop drawable {name} is invalid: {value}"
        )));
    }
    Ok(value.round() as usize)
}

fn is_main_thread() -> bool {
    // SAFETY: pthread_main_np has no preconditions and only reports the calling thread identity.
    unsafe { libc::pthread_main_np() == 1 }
}

fn milkdrop_view_class() -> &'static Class {
    static CLASS: OnceLock<usize> = OnceLock::new();
    let address = *CLASS.get_or_init(|| {
        if let Some(class) = Class::get("AMP808MilkdropOpenGLView") {
            return class as *const Class as usize;
        }
        let mut declaration = ClassDecl::new("AMP808MilkdropOpenGLView", class!(NSOpenGLView))
            .expect("AMP808 MilkDrop view class has a unique Objective-C name");
        // SAFETY: each callback uses the exact Objective-C encoding implied by its selector and
        // returns no borrowed Rust data.
        unsafe {
            declaration.add_method(
                sel!(hitTest:),
                hit_test as extern "C" fn(&Object, Sel, NSPoint) -> id,
            );
            declaration.add_method(
                sel!(acceptsFirstResponder),
                accepts_first_responder as extern "C" fn(&Object, Sel) -> BOOL,
            );
            declaration.add_method(
                sel!(isAccessibilityElement),
                is_accessibility_element as extern "C" fn(&Object, Sel) -> BOOL,
            );
        }
        declaration.register() as *const Class as usize
    });
    // SAFETY: OnceLock stores the address of a permanently registered Objective-C class.
    unsafe { &*(address as *const Class) }
}

extern "C" fn hit_test(_view: &Object, _selector: Sel, _point: NSPoint) -> id {
    nil
}

extern "C" fn accepts_first_responder(_view: &Object, _selector: Sel) -> BOOL {
    NO
}

extern "C" fn is_accessibility_element(_view: &Object, _selector: Sel) -> BOOL {
    NO
}

#[cfg(test)]
mod tests {
    use super::appkit_frame;

    #[test]
    fn appkit_frame_flips_gpui_top_left_coordinates() {
        let frame = appkit_frame(20.0, 30.0, 400.0, 200.0, 720.0);

        assert_eq!(frame.origin.x, 20.0);
        assert_eq!(frame.origin.y, 490.0);
        assert_eq!(frame.size.width, 400.0);
        assert_eq!(frame.size.height, 200.0);
    }
}
