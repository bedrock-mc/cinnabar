#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BedrockVersionInfo {
    pub minecraft_version: &'static str,
    pub protocol_version: i32,
    pub major_version: &'static str,
    pub release_type: &'static str,
}
