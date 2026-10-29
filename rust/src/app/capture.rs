//! Reading back the frame as drawn, for `--snap` and `--record`.

use std::error::Error;

use super::*;
use crate::eye::{self, EyeState};
use crate::gpu::Gpu;
use crate::snap::{alpha_census, write_snap};

impl App {
    pub(super) fn snap(&self, gpu: &mut Gpu, state: &EyeState) -> Result<(), Box<dyn Error>> {
        let size = gpu.size();
        let rgba = self.grab(gpu, state);
        write_snap("snap.png", size, &rgba)?;
        let (clear, partial, solid) = alpha_census(&rgba);
        eprintln!(
            "snap.png written: {}x{}, alpha clear {clear} / partial {partial} / solid {solid} px",
            size.0, size.1
        );
        Ok(())
    }

    /// The frame as drawn (eye and whispers), read back as premultiplied RGBA.
    pub(super) fn grab(&self, gpu: &mut Gpu, state: &EyeState) -> Vec<u8> {
        let size = gpu.size();
        let scale = gpu.window.scale_factor() as f32;
        eye::read_frame(
            &gpu.device,
            &gpu.queue,
            &gpu.eye,
            gpu.config.format,
            size,
            state,
            |encoder, view| {
                gpu.whispers.draw(
                    &gpu.device,
                    &gpu.queue,
                    encoder,
                    view,
                    size,
                    scale,
                    state.colors,
                    state.fade,
                )
            },
        )
    }

    /// --record: writes out the GIF, once the event loop has ended.
    pub(crate) fn finish_recording(&mut self) -> Result<(), Box<dyn Error>> {
        if let Some(recording) = self.recording.take() {
            recording.finish()?;
        }
        Ok(())
    }
}
