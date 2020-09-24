use rustc_version::*;
use std::env;

fn transfer_env(var: &str) {
    if let Ok(value) = env::var(var) {
        println!("cargo:rustc-env={}={}", var, value);
    }
}
fn main() {
    transfer_env("PROFILE");
    transfer_env("TARGET");
    transfer_env("HOST");

    if let Ok(version) = version_meta() {
        println!("cargo:rustc-env=RUSTC_VERSION_STR={}", version.short_version_string);
    } else {
        println!("cargo:rustc-env=RUSTC_VERSION_STR=unknown");
    }
}