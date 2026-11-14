use std::fs;
use std::process::Command;

/// Bundle the editor's formatter so installed binaries do not depend on a checkout.
pub fn run(args: &[String]) -> i32 {
    match launch(args) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("ube format: {error}");
            2
        }
    }
}

fn launch(args: &[String]) -> Result<i32, String> {
    let directory = tempfile::Builder::new()
        .prefix("ube-fmt-")
        .tempdir()
        .map_err(|error| error.to_string())?;
    let bin = directory.path().join("bin");
    fs::create_dir(&bin).map_err(|error| error.to_string())?;
    fs::write(
        directory.path().join("formatter.js"),
        include_str!("../../editors/vscode/formatter.js"),
    )
    .map_err(|error| error.to_string())?;
    let script = bin.join("fmt.js");
    fs::write(&script, include_str!("../../editors/vscode/bin/fmt.js"))
        .map_err(|error| error.to_string())?;
    // Inherit cwd and all streams so relative paths and stdin work normally.
    let status = Command::new("node")
        .arg(script)
        .args(args)
        .status()
        .map_err(|error| format!("could not run Node.js (version 18+ required): {error}"))?;
    Ok(status.code().unwrap_or(2))
}
