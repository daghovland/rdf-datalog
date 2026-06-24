use std::path::PathBuf;

use dagalog_kernel::sockets;

/// Install the kernel spec into `~/.local/share/jupyter/kernels/dagalog/`.
fn install_kernel(user: bool) -> Result<(), String> {
    let base = if user {
        dirs::home_dir()
            .ok_or("cannot find home directory")?
            .join(".local/share/jupyter/kernels/dagalog")
    } else {
        PathBuf::from("/usr/local/share/jupyter/kernels/dagalog")
    };

    std::fs::create_dir_all(&base)
        .map_err(|e| format!("cannot create kernel dir {}: {}", base.display(), e))?;

    let exe =
        std::env::current_exe().map_err(|e| format!("cannot determine executable path: {}", e))?;

    let kernel_json = serde_json::json!({
        "argv": [exe.to_string_lossy(), "launch", "--connection-file", "{connection_file}"],
        "display_name": "Dagalog (SPARQL + RDF)",
        "language": "sparql"
    });

    let kernel_json_path = base.join("kernel.json");
    std::fs::write(
        &kernel_json_path,
        serde_json::to_string_pretty(&kernel_json).unwrap(),
    )
    .map_err(|e| format!("cannot write {}: {}", kernel_json_path.display(), e))?;

    println!("Installed kernel spec to {}", base.display());
    println!("Run: jupyter kernelspec list");
    Ok(())
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let result = match args.get(1).map(|s| s.as_str()) {
        Some("install") => {
            let user = !args.contains(&"--sys-prefix".to_string());
            install_kernel(user)
        }
        Some("launch") => {
            let conn_file = args
                .iter()
                .position(|a| a == "--connection-file")
                .and_then(|i| args.get(i + 1))
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("connection.json"));
            sockets::run_kernel(&conn_file).await
        }
        _ => {
            eprintln!("Usage: dagalog-kernel install | launch --connection-file <path>");
            std::process::exit(1);
        }
    };

    if let Err(e) = result {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}
