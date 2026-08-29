// Explorer launches must never allocate a console, including local-review
// Debug packages. Runtime and broker diagnostics belong in bounded app-owned
// logs; they are supervised hidden children rather than visible consoles.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    interactive_npcs_control_lib::run();
}
