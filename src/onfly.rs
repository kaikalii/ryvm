use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use ropey::Rope;
use ryvm_spec::{DynamicValue, Name, Omitted};

use crate::{Control, RyvmResult};

#[derive(Debug, Clone)]
pub struct FlyControl {
    pub file: PathBuf,
    pub channel: Option<u8>,
    pub index: usize,
    pub rope: Rope,
    start: Option<Instant>,
}

const FLY_PATTERN: &str = "#";

impl FlyControl {
    pub fn find<P>(path: P, channel: Option<u8>, delay: bool) -> RyvmResult<Option<Self>>
    where
        P: AsRef<Path>,
    {
        let file_str = fs::read_to_string(&path)?;
        let index = if let Some(i) = file_str.find(FLY_PATTERN) {
            i
        } else {
            return Ok(None);
        };
        let rope = Rope::from_str(&file_str);
        let index = rope.byte_to_char(index);
        Ok(Some(FlyControl {
            file: path.as_ref().into(),
            index,
            rope,
            channel,
            start: if delay { Some(Instant::now()) } else { None },
        }))
    }
    /// Try to process a control and return whether it was mapped
    pub fn process(
        &mut self,
        control: Control,
        mut name: impl FnMut() -> Option<Name>,
    ) -> RyvmResult<bool> {
        if self
            .start
            .map_or(false, |start| (Instant::now() - start).as_secs_f32() < 1.0)
        {
            return Ok(false);
        }
        if let Control::Control(i, _) = control {
            // Create control value
            let value = DynamicValue::Control {
                controller: name().into(),
                number: i,
                bounds: Omitted,
            };
            // Serialize control value
            let mut config = ron::ser::PrettyConfig::default();
            config.new_line = " ".into();
            config.indentor = "".into();
            let mut value_str = ron::ser::to_string_pretty(&value, config)?;
            value_str.push(',');
            // Insert control string
            self.rope
                .remove(self.index..(self.index + FLY_PATTERN.len()));
            self.rope.insert(self.index, &value_str);
            // Write the file
            let file = fs::File::create(&self.file)?;
            self.rope.write_to(file)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
