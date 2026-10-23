use experience_sdk::{entry::server, server::Experience};

struct TerminalServer;

#[server]
impl Experience for TerminalServer {}
