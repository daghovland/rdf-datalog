/// ZMQ socket setup and main message loop.
///
/// Reads a Jupyter connection file, binds five ZMQ sockets, and dispatches
/// messages to the session. All implementation is in the green phase.

#[derive(Debug, serde::Deserialize)]
pub struct ConnectionFile {
    pub ip: String,
    pub transport: String,
    pub shell_port: u16,
    pub iopub_port: u16,
    pub stdin_port: u16,
    pub control_port: u16,
    pub hb_port: u16,
    pub key: String,
    pub signature_scheme: String,
    pub kernel_name: String,
}

pub async fn run_kernel(_connection_file_path: &std::path::Path) -> Result<(), String> {
    todo!("run_kernel: bind ZMQ sockets and start message loop")
}
