#![expect(
    deprecated,
    reason = "ADR-0025 isolates the existing cocoa/objc AppKit bridge inside the macOS adapter"
)]

use std::ffi::c_void;
use std::mem;

use cocoa::appkit::NSView;
use cocoa::base::{NO, YES, id, nil};
use core_graphics_types::geometry::CGSize;
use gpui_kit::{Bounds, Pixels, Window, WindowCompositionSurface};
use metal::{
    CompileOptions, Device, MTLLoadAction, MTLPixelFormat, MTLPrimitiveType, MTLStoreAction,
    MetalLayer, RenderPassDescriptor, RenderPipelineDescriptor, RenderPipelineState,
};
use objc::{class, msg_send, sel, sel_impl};

use super::{CosmicError, CosmicFrame};

const COSMIC_SHADER: &str = include_str!("cosmic.metal");

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct CosmicUniforms {
    resolution: [f32; 2],
    time_seconds: f32,
    bass: f32,
    mid: f32,
    high: f32,
    rms: f32,
    peak: f32,
}

impl CosmicUniforms {
    fn new(width: u64, height: u64, frame: CosmicFrame) -> Self {
        let signal = frame.signal();
        Self {
            resolution: [width as f32, height as f32],
            time_seconds: frame.time_seconds(),
            bass: signal.bass(),
            mid: signal.mid(),
            high: signal.high(),
            rms: signal.rms(),
            peak: signal.peak(),
        }
    }
}

pub(super) struct CosmicMetalView {
    surface: WindowCompositionSurface,
    layer: MetalLayer,
    command_queue: metal::CommandQueue,
    pipeline: RenderPipelineState,
}

impl CosmicMetalView {
    pub(super) fn attach(window: &mut Window) -> Result<Self, CosmicError> {
        if !is_main_thread() {
            return Err(CosmicError::backend(
                "native Metal surface must be created on the macOS main thread",
            ));
        }

        let device = Device::system_default()
            .ok_or_else(|| CosmicError::backend("macOS did not provide a Metal device"))?;
        let library = device
            .new_library_with_source(COSMIC_SHADER, &CompileOptions::new())
            .map_err(|error| CosmicError::backend(format!("compiling cosmic shader: {error}")))?;
        let vertex = library
            .get_function("cosmic_vertex", None)
            .map_err(|error| {
                CosmicError::backend(format!("loading cosmic vertex shader: {error}"))
            })?;
        let fragment = library
            .get_function("cosmic_fragment", None)
            .map_err(|error| {
                CosmicError::backend(format!("loading cosmic fragment shader: {error}"))
            })?;
        let descriptor = RenderPipelineDescriptor::new();
        descriptor.set_label("AMP808 cosmic background");
        descriptor.set_vertex_function(Some(&vertex));
        descriptor.set_fragment_function(Some(&fragment));
        descriptor
            .color_attachments()
            .object_at(0)
            .expect("Metal render pipeline exposes color attachment zero")
            .set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        let pipeline = device
            .new_render_pipeline_state(&descriptor)
            .map_err(|error| CosmicError::backend(format!("creating cosmic pipeline: {error}")))?;
        let command_queue = device.new_command_queue();

        let composition = window
            .enable_window_composition()
            .map_err(|error| CosmicError::backend(format!("enabling GPUI composition: {error}")))?;
        let surface = composition.create_native_surface().map_err(|error| {
            CosmicError::backend(format!("creating GPUI native surface: {error}"))
        })?;
        let surface_id = surface.id();
        let attach_result = (|| {
            let handle = surface.platform_surface().map_err(|error| {
                CosmicError::backend(format!("accessing GPUI native surface: {error}"))
            })?;
            let view_handle = handle.platform_handle().map_err(|error| {
                CosmicError::backend(format!("accessing AppKit surface handle: {error}"))
            })?;
            let view_pointer = view_handle.downcast::<usize>().map_err(|_| {
                CosmicError::backend("GPUI native surface is not backed by an AppKit NSView")
            })?;
            let view = *view_pointer as id;
            if view == nil {
                return Err(CosmicError::backend("GPUI returned a null AppKit NSView"));
            }

            let mut layer = MetalLayer::new();
            layer.set_device(&device);
            layer.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
            layer.set_framebuffer_only(true);
            layer.set_opaque(true);
            layer.set_display_sync_enabled(true);
            layer.set_maximum_drawable_count(3);

            // SAFETY: GPUI owns a live NSView on AppKit's main thread. AppKit retains the Metal
            // layer after `setLayer:` while this adapter also retains its Rust owner.
            unsafe {
                NSView::setWantsLayer(view, YES);
                NSView::setLayer(view, <*mut _>::cast(layer.as_mut()));
                let _: () = msg_send![view, setAccessibilityElement: NO];
            }
            Ok(layer)
        })();
        let layer = match attach_result {
            Ok(layer) => layer,
            Err(error) => {
                let _ = composition.remove_surface(surface_id);
                return Err(error);
            }
        };

        Ok(Self {
            surface,
            layer,
            command_queue,
            pipeline,
        })
    }

    pub(super) fn present(
        &mut self,
        bounds: Bounds<Pixels>,
        scale_factor: f32,
        frame: CosmicFrame,
    ) -> Result<bool, CosmicError> {
        let device_bounds = bounds.to_device_pixels(scale_factor);
        self.surface
            .platform_surface()
            .and_then(|surface| surface.set_bounds(device_bounds))
            .map_err(|error| CosmicError::backend(format!("resizing cosmic surface: {error}")))?;

        let width = u64::try_from(device_bounds.size.width.0.max(0))
            .map_err(|_| CosmicError::backend("cosmic surface width is invalid"))?;
        let height = u64::try_from(device_bounds.size.height.0.max(0))
            .map_err(|_| CosmicError::backend("cosmic surface height is invalid"))?;
        if width == 0 || height == 0 {
            return Ok(false);
        }
        self.layer
            .set_drawable_size(CGSize::new(width as f64, height as f64));

        let uniforms = CosmicUniforms::new(width, height, frame);
        let Some(drawable) = self.layer.next_drawable() else {
            return Ok(false);
        };
        let pass = RenderPassDescriptor::new();
        let color = pass
            .color_attachments()
            .object_at(0)
            .expect("Metal render pass exposes color attachment zero");
        color.set_texture(Some(drawable.texture()));
        color.set_load_action(MTLLoadAction::Clear);
        color.set_clear_color(metal::MTLClearColor::new(0.005, 0.003, 0.02, 1.0));
        color.set_store_action(MTLStoreAction::Store);

        let command_buffer = self.command_queue.new_command_buffer();
        command_buffer.set_label("AMP808 cosmic frame");
        let encoder = command_buffer.new_render_command_encoder(pass);
        encoder.set_label("AMP808 cosmic encoder");
        encoder.set_render_pipeline_state(&self.pipeline);
        encoder.set_fragment_bytes(
            0,
            mem::size_of::<CosmicUniforms>() as u64,
            (&raw const uniforms).cast::<c_void>(),
        );
        encoder.draw_primitives(MTLPrimitiveType::Triangle, 0, 3);
        encoder.end_encoding();
        command_buffer.present_drawable(drawable);
        command_buffer.commit();
        Ok(true)
    }

    pub(super) fn hide(&self) {
        if let Ok(surface) = self.surface.platform_surface() {
            let _ = surface.set_visible(false);
        }
    }
}

fn is_main_thread() -> bool {
    // SAFETY: `NSThread.isMainThread` is a process-global query with no object lifetime input.
    unsafe { msg_send![class!(NSThread), isMainThread] }
}

impl Drop for CosmicMetalView {
    fn drop(&mut self) {
        self.hide();
    }
}
