#[derive(Debug, Clone)]
pub struct Script {
    pub name: String,
    pub arguments: Vec<String>,
    pub unresolved_commands: Vec<(bool, Vec<String>)>,
}
