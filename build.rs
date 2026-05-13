use std::{env, fs, path::Path, process::Command};

fn main() {
	println!("cargo:rerun-if-changed=data/swayosd.gresource.xml");
	print_rerun_if_changed(Path::new("data/icons"));

	let output = Command::new("glib-compile-resources")
		.args(["./data/swayosd.gresource.xml", "--sourcedir=./data"])
		.arg(format!(
			"--target={}/swayosd.gresource",
			env::var("OUT_DIR").unwrap()
		))
		.status()
		.expect("failed to execute process");
	assert!(output.success());
}

fn print_rerun_if_changed(path: &Path) {
	let Ok(entries) = fs::read_dir(path) else {
		return;
	};

	for entry in entries.flatten() {
		let path = entry.path();
		if path.is_dir() {
			print_rerun_if_changed(&path);
		} else if path.is_file() {
			println!("cargo:rerun-if-changed={}", path.display());
		}
	}
}
