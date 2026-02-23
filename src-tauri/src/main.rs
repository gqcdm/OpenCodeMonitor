// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::thread;

fn main() {
    // Sync PATH from shell in background - don't block app launch
    thread::spawn(|| {
        if let Err(err) = fix_path_env::fix() {
            eprintln!("Failed to sync PATH from shell: {err}");
        }
    });

    opencode_monitor_lib::run()
}
