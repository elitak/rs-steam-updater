// ... existing content above

// Add verbose logging to the launch_steam function

fn launch_steam() {
    let exe_path = "path_to_exe"; // Replace with the actual executable path
    println!("Resolved exe path: {}", exe_path);

    // Check if the executable exists
    if !std::path::Path::new(exe_path).exists() {
        println!("Error: Executable does not exist at path: {}", exe_path);
        return;
    }

    // Spawn the process
    let child = std::process::Command::new(exe_path)
        .spawn();

    match child {
        Ok(child_process) => {
            println!("Child process started with PID: {}", child_process.id());
        }
        Err(e) => {
            println!("Failed to start process: {}", e);
        }
    }
}