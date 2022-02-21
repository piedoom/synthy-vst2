use std::sync::Arc;

use crate::{Parameters, PluginEditor, VstParent};
use baseview::*;
use egui::*;
use egui_baseview::*;
use vst::editor::Editor;

const WINDOW_WIDTH: usize = 512;
const WINDOW_HEIGHT: usize = 512;

impl Editor for PluginEditor {
    fn position(&self) -> (i32, i32) {
        (0, 0)
    }

    fn size(&self) -> (i32, i32) {
        (WINDOW_WIDTH as i32, WINDOW_HEIGHT as i32)
    }

    fn open(&mut self, parent: *mut ::std::ffi::c_void) -> bool {
        log::info!("Editor open");
        if self.is_open {
            return false;
        }

        self.is_open = true;

        let settings = Settings {
            window: WindowOpenOptions {
                title: String::from("synthy"),
                size: Size::new(WINDOW_WIDTH as f64, WINDOW_HEIGHT as f64),
                scale: WindowScalePolicy::SystemScaleFactor,
            },
            render_settings: RenderSettings::default(),
        };

        let window_handle = EguiWindow::open_parented(
            &VstParent(parent),
            settings,
            self.params.clone(),
            |_egui_ctx: &CtxRef, _queue: &mut Queue, _state: &mut Arc<Parameters>| {},
            |egui_ctx: &CtxRef, _queue: &mut Queue, state: &mut Arc<Parameters>| {
                egui::Window::new("synthy").show(&egui_ctx, |ui| {
                    let mut val = state.pan.get();
                    if ui
                        .add(egui::Slider::new(&mut val, 0.0..=1.0).text("Pan"))
                        .changed()
                    {
                        state.pan.set(val)
                    }
                });
            },
        );

        self.window_handle = Some(window_handle);

        true
    }

    fn is_open(&mut self) -> bool {
        self.is_open
    }

    fn close(&mut self) {
        self.is_open = false;
        if let Some(mut window_handle) = self.window_handle.take() {
            window_handle.close();
        }
    }
}
