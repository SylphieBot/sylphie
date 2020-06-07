extern crate git2;
extern crate rustc_version;

use std::env;
use rustc_version::*;

fn transfer_env(var: &str) {
    if let Ok(value) = env::var(var) {
        println!("cargo:rustc-env={}={}", var, value);
    }
}
fn main() {
    transfer_env("PROFILE");
    transfer_env("TARGET");
    transfer_env("HOST");

    if let Ok(repo) = git2::Repository::discover(".") {
        println!("cargo:rustc-env=GIT_COMMIT={}",
                 repo.head().unwrap().resolve().unwrap().target().unwrap());
        let statuses = repo.statuses(Some(git2::StatusOptions::new()
            .include_untracked(true)
            .recurse_untracked_dirs(true)
            .include_ignored(false)
            .sort_case_insensitively(true))).unwrap();
        if !statuses.is_empty() {
            println!("cargo:rustc-env=GIT_IS_DIRTY=true");
        }
    } else {
        println!("cargo:rustc-env=GIT_COMMIT=unknown");
    }

    if let Ok(version) = version_meta() {
        println!("cargo:rustc-env=RUSTC_VERSION_STR={}", version.short_version_string);
    } else {
        println!("cargo:rustc-env=RUSTC_VERSION_STR=unknown");
    }
}