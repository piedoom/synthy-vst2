//! Barebones baseview egui plugin

mod editor;

#[macro_use]
extern crate vst;

use baseview::WindowHandle;
use fundsp::hacker::*;
use raw_window_handle::{HasRawWindowHandle, RawWindowHandle};
use vst::buffer::AudioBuffer;
use vst::editor::Editor;
use vst::plugin::{Category, HostCallback, Info, Plugin, PluginParameters};
use vst::util::AtomicFloat;
use wmidi::Note;

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct PluginEditor {
    params: Arc<Parameters>,
    window_handle: Option<WindowHandle>,
    is_open: bool,
}

struct Parameters {
    // The plugin's state consists of a single parameter: amplitude.
    pan: AtomicFloat,
    // Whether or not a parameter has been changed since last update
    dirty: AtomicBool,
}

struct Synthy {
    sample_rate: f32,
    graph: Box<dyn AudioUnit64>,
    params: Arc<Parameters>,
    editor: Option<PluginEditor>,
    notes: VecDeque<(Note, Velocity)>,
}

type Velocity = u8;

impl Default for Synthy {
    fn default() -> Self {
        // construct parameters
        let params = Arc::new(Parameters::default());

        // construct a temporary audio graph. We'll actually update this later.
        let c = pulse();

        Self {
            sample_rate: 48_000.,
            params: params.clone(),
            editor: Some(PluginEditor {
                params,
                window_handle: None,
                is_open: false,
            }),
            graph: Box::new(c),
            notes: VecDeque::with_capacity(1),
        }
    }
}

impl Default for Parameters {
    fn default() -> Parameters {
        Parameters {
            pan: AtomicFloat::new(0.0),
            dirty: AtomicBool::new(true),
        }
    }
}

impl Plugin for Synthy {
    fn new(_host: HostCallback) -> Self {
        Self::default()
    }

    fn get_info(&self) -> Info {
        Info {
            name: "synthy".to_string(),
            vendor: "doomy".to_string(),
            unique_id: 243123013,
            version: 1,
            inputs: 0,
            outputs: 2,
            f64_precision: false,
            // This `parameters` bit is important; without it, none of our
            // parameters will be shown!
            parameters: 1,
            category: Category::Synth,
            ..Default::default()
        }
    }

    fn init(&mut self) {
        // Set up logs
        let log_folder = dirs::home_dir().unwrap().join("tmp");
        let _ = std::fs::create_dir(log_folder.clone());
        let Info {
            name,
            version,
            unique_id,
            ..
        } = self.get_info();
        let id_string = format!("{name}-{version}-{unique_id}-log.txt");
        let log_file = ::std::fs::File::create(log_folder.join(id_string))
            .expect("could not write to log file");
        let log_config = ::simplelog::ConfigBuilder::new()
            .set_time_to_local(true)
            .build();
        let _ = simplelog::WriteLogger::init(simplelog::LevelFilter::Info, log_config, log_file);
        log_panics::init();
        log::info!("init");
    }

    fn get_editor(&mut self) -> Option<Box<dyn Editor>> {
        if let Some(editor) = self.editor.take() {
            Some(Box::new(editor) as Box<dyn Editor>)
        } else {
            None
        }
    }

    fn set_sample_rate(&mut self, rate: f32) {
        self.sample_rate = rate
    }

    // Here is where the bulk of our audio processing code goes.
    fn process(&mut self, buffer: &mut AudioBuffer<f32>) {
        let output_count = buffer.output_count();
        let (_inputs, mut outputs) = buffer.split();
        if output_count == 2 {
            let (l, r) = (outputs.get_mut(0), outputs.get_mut(1));
            // process by 64 sized blocks, which is the max for fundsp
            for (l_chunk, r_chunk) in l.chunks_mut(64).zip(r.chunks_mut(64)) {
                let mut out_buffer_l = [0f64; 64];
                let mut out_buffer_r = [0f64; 64];

                self.graph
                    .process(64, &[], &mut [&mut out_buffer_l, &mut out_buffer_r]);

                for (chunk, output) in l_chunk.iter_mut().zip(out_buffer_l.iter()) {
                    *chunk = *output as f32;
                }

                for (chunk, output) in r_chunk.iter_mut().zip(out_buffer_r.iter()) {
                    *chunk = *output as f32;
                }
            }
        }
    }

    // Return the parameter object. This method can be omitted if the
    // plugin has no parameters.
    fn get_parameter_object(&mut self) -> Arc<dyn PluginParameters> {
        Arc::clone(&self.params) as Arc<dyn PluginParameters>
    }

    fn process_events(&mut self, events: &vst::api::Events) {
        for event in events.events() {
            match event {
                vst::event::Event::Midi(e) => {
                    if let Ok(msg) = wmidi::MidiMessage::try_from(e.data.as_slice()) {
                        match msg {
                            wmidi::MidiMessage::NoteOff(_, n, _) => {
                                self.notes = self
                                    .notes
                                    .iter()
                                    .filter_map(
                                        |(a, v)| {
                                            if a != &n {
                                                Some((*a, *v))
                                            } else {
                                                None
                                            }
                                        },
                                    )
                                    .collect();
                            }
                            wmidi::MidiMessage::NoteOn(_, n, v) => {
                                self.notes.push_back((n, v.into()));
                            }
                            _ => (),
                        }
                    }
                }
                vst::event::Event::SysEx(_) => (),
                vst::event::Event::Deprecated(_) => (),
            }
        }
    }

    fn start_process(&mut self) {
        if self.params.dirty.swap(false, Ordering::Relaxed) {
            self.update_audio_graph();
        }
    }
}

impl PluginParameters for Parameters {
    // the `get_parameter` function reads the value of a parameter.
    fn get_parameter(&self, index: i32) -> f32 {
        match index {
            0 => self.pan.get(),
            _ => 0.0,
        }
    }

    // the `set_parameter` function sets the value of a parameter.
    fn set_parameter(&self, index: i32, val: f32) {
        #[allow(clippy::single_match)]
        match index {
            0 => self.pan.set(val),
            _ => (),
        }

        // regenerate audio graph as things changed
        self.dirty.store(true, Ordering::Relaxed);
    }

    // This is what will display underneath our control.  We can
    // format it into a string that makes the most since.
    fn get_parameter_text(&self, index: i32) -> String {
        match index {
            0 => format!("{:.2}", (self.pan.get() - 0.5) * 2f32),
            _ => "".to_string(),
        }
    }

    // This shows the control's name.
    fn get_parameter_name(&self, index: i32) -> String {
        match index {
            0 => "Amplitude",
            _ => "",
        }
        .to_string()
    }
}

impl Synthy {
    fn update_audio_graph(&mut self) {
        let pan = self.params.pan.get();

        let graph = lfo(move |t| {
            let pitch = 110.0;
            let duty = lerp11(0.01, 0.99, sin_hz(0.05, t));
            (pitch, duty)
        }) >> pulse()
            >> declick()
            >> hacker::pan((pan - 0.5f32) as f64);

        self.graph = Box::new(graph);
    }
}

struct VstParent(*mut ::std::ffi::c_void);

#[cfg(target_os = "macos")]
unsafe impl HasRawWindowHandle for VstParent {
    fn raw_window_handle(&self) -> RawWindowHandle {
        use raw_window_handle::macos::MacOSHandle;

        RawWindowHandle::MacOS(MacOSHandle {
            ns_view: self.0 as *mut ::std::ffi::c_void,
            ..MacOSHandle::empty()
        })
    }
}

#[cfg(target_os = "windows")]
unsafe impl HasRawWindowHandle for VstParent {
    fn raw_window_handle(&self) -> RawWindowHandle {
        use raw_window_handle::windows::WindowsHandle;

        RawWindowHandle::Windows(WindowsHandle {
            hwnd: self.0,
            ..WindowsHandle::empty()
        })
    }
}

#[cfg(target_os = "linux")]
unsafe impl HasRawWindowHandle for VstParent {
    fn raw_window_handle(&self) -> RawWindowHandle {
        use raw_window_handle::unix::XcbHandle;

        RawWindowHandle::Xcb(XcbHandle {
            window: self.0 as u32,
            ..XcbHandle::empty()
        })
    }
}

plugin_main!(Synthy);

// fn construct_audio_graph() -> impl AudioNode {
//     // Pulse wave.
//     let c = lfo(|t| {
//         let pitch = 110.0;
//         let duty = lerp11(0.01, 0.99, sin_hz(0.05, t));
//         (pitch, duty)
//     }) >> pulse();
//     c
// }
