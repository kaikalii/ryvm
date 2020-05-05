use std::{sync::Arc, thread};

use cpal::{
    traits::{DeviceTrait, EventLoopTrait, HostTrait},
    StreamData, UnknownTypeInputBuffer,
};

use crate::CloneLock;

pub struct AudioInput {
    buffer: Arc<CloneLock<Vec<f32>>>,
}

impl AudioInput {
    pub fn _new() -> Self {
        let host = cpal::default_host();
        let event_loop = host.event_loop();
        let device = host
            .default_input_device()
            .expect("No available sound input devices");
        let format = device
            .supported_output_formats()
            .expect("error while querying formats")
            .next()
            .expect("no supported format?!")
            .with_max_sample_rate();
        let stream_id = event_loop.build_input_stream(&device, &format).unwrap();
        event_loop
            .play_stream(stream_id)
            .expect("failed to play_stream");

        println!("{:?}", format);

        let buffer = Arc::new(CloneLock::new(Vec::new()));
        let buffer_clone = Arc::clone(&buffer);

        thread::spawn(move || {
            let buffer = buffer_clone;
            event_loop.run(move |stream_id, stream_result| {
                let stream_data = match stream_result {
                    Ok(data) => data,
                    Err(err) => {
                        eprintln!("an error occurred on stream {:?}: {}", stream_id, err);
                        return;
                    }
                };
                let mut buffer = buffer.lock();
                let in_buf: Vec<f32> = match stream_data {
                    StreamData::Input {
                        buffer: UnknownTypeInputBuffer::U16(in_buf),
                    } => in_buf
                        .iter()
                        .map(|s| (*s as f32 / (u16::MAX as f32 / 2.0)) - 1.0)
                        .collect(),
                    StreamData::Input {
                        buffer: UnknownTypeInputBuffer::I16(in_buf),
                    } => in_buf
                        .iter()
                        .map(|s| (*s as f32 / (i16::MAX as f32 / 2.0)))
                        .collect(),
                    StreamData::Input {
                        buffer: UnknownTypeInputBuffer::F32(in_buf),
                    } => in_buf.iter().copied().collect(),
                    _ => Vec::new(),
                };
                for s in &in_buf {
                    println!("{}", s);
                }
                buffer.extend(in_buf)
            })
        });
        AudioInput { buffer }
    }
}
