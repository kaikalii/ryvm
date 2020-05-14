use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    thread::Builder,
};

use cpal::{
    traits::{DeviceTrait, EventLoopTrait, HostTrait},
    DeviceNameError, DevicesError, EventLoop, Format, Host, PlayStreamError, SampleRate,
    StreamData, StreamId, SupportedFormatsError, UnknownTypeInputBuffer,
};
use crossbeam_channel::{unbounded, Receiver, Sender};
use itertools::Itertools;
use thiserror::Error;

use crate::{colorprintln, ty::Voice, utility::SampleRange};

#[derive(Debug, Error)]
pub enum InputError {
    #[error("Input device name error: {0}")]
    DeviceName(#[from] DeviceNameError),
    #[error("Input devices error: {0}")]
    Devices(#[from] DevicesError),
    #[error("Error playing stream: {0}")]
    PlayStream(#[from] PlayStreamError),
    #[error("Supported formats error {0}")]
    Supported(#[from] SupportedFormatsError),
    #[error("Device supports no formats")]
    NoFormats,
    #[error("Unknown device: {0}")]
    UnknownDevice(String),
    #[error("No default audio input device")]
    NoDefaultInput,
}

type StreamFrame = f32;

type NewInputDevice = (StreamId, Sender<Vec<StreamFrame>>);

pub struct InputManager {
    host: Arc<Host>,
    event_loop: Arc<EventLoop>,
    send_send: Sender<NewInputDevice>,
}

impl InputManager {
    pub fn new() -> Self {
        let host = Arc::new(cpal::default_host());
        let event_loop = Arc::new(host.event_loop());

        let event_loop_clone = Arc::clone(&event_loop);

        // Channel for sending device data senders to the event loop thread
        let (send_send, send_recv) = unbounded::<NewInputDevice>();

        Builder::new()
            .name("audio input".into())
            .spawn(move || {
                let send_recv = send_recv;
                let mut senders = HashMap::new();
                event_loop_clone.run(move |stream_id, stream_result| {
                    // Get new device connections
                    for (id, sender) in send_recv.try_iter() {
                        senders.insert(id, sender);
                    }

                    // Unwrap stream data
                    let stream_data = match stream_result {
                        Ok(data) => data,
                        Err(err) => {
                            colorprintln!(
                                "An error occurred on stream {:?}: {}.",
                                bright_red,
                                stream_id,
                                err
                            );
                            return;
                        }
                    };

                    // Convert stream data
                    if let Some(sender) = senders.get(&stream_id) {
                        let buffer: Vec<StreamFrame> = match stream_data {
                            StreamData::Output { .. } => panic!("input stream using output buffer"),
                            StreamData::Input { buffer } => match buffer {
                                UnknownTypeInputBuffer::U16(buffer) => {
                                    buffer.iter().copied().map(SampleRange::to_f32).collect()
                                }
                                UnknownTypeInputBuffer::I16(buffer) => {
                                    buffer.iter().copied().map(SampleRange::to_f32).collect()
                                }
                                UnknownTypeInputBuffer::F32(buffer) => {
                                    buffer.iter().copied().collect()
                                }
                            },
                        };
                        // Send stream data to device interface
                        let _ = sender.send(buffer);
                    }
                })
            })
            .expect("failed to spawn audio input thread");

        InputManager {
            host: Arc::clone(&host),
            event_loop: Arc::clone(&event_loop),
            send_send,
        }
    }
    pub fn add_device(
        &self,
        name: Option<String>,
        sample_rate: u32,
    ) -> Result<InputDevice, InputError> {
        let device = if let Some(name) = name {
            self.host
                .input_devices()?
                .find(|dev| dev.name().unwrap().contains(&name))
                .ok_or_else(|| InputError::UnknownDevice(name))?
        } else {
            self.host
                .default_input_device()
                .ok_or(InputError::NoDefaultInput)?
        };
        let mut format = if let Some(format) = device
            .supported_input_formats()?
            .find(|format| format.channels == 2)
        {
            Some(format)
        } else {
            device.supported_input_formats()?.next()
        }
        .ok_or(InputError::NoFormats)?
        .with_max_sample_rate();
        format.sample_rate = SampleRate(sample_rate);
        let stream_id = self
            .event_loop
            .build_input_stream(&device, &format)
            .unwrap();
        self.event_loop.play_stream(stream_id.clone())?;
        let (send, recv) = unbounded();
        let _ = self.send_send.send((stream_id, send));
        Ok(InputDevice {
            device: device.name()?,
            recv: Arc::new(recv),
            format,
            queue: VecDeque::new(),
        })
    }
}

#[derive(Debug)]
pub struct InputDevice {
    device: String,
    recv: Arc<Receiver<Vec<f32>>>,
    format: Format,
    queue: VecDeque<Voice>,
}

impl InputDevice {
    pub fn device(&self) -> &str {
        &self.device
    }
    pub fn sample(&mut self) -> Option<Voice> {
        for buffer in self.recv.try_iter() {
            match self.format.channels {
                1 => self.queue.extend(buffer.into_iter().map(Voice::mono)),
                2 => self.queue.extend(
                    buffer
                        .into_iter()
                        .tuples()
                        .map(|(l, r)| Voice::stereo(l, r)),
                ),
                _ => panic!("weird default input device channel count"),
            }
        }
        self.queue.pop_front()
    }
}
