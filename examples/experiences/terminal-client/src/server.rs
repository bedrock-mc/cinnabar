use experience_sdk::server::Experience;

struct TerminalServer;

#[experience_sdk::server]
impl Experience for TerminalServer {}
