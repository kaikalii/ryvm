pub struct Controller {
    pub window_size: [f64; 2],
    pub mouse_pos: [f64; 2],
}

impl Default for Controller {
    fn default() -> Self {
        Controller {
            window_size: [800.0; 2],
            mouse_pos: [0.0; 2],
        }
    }
}
