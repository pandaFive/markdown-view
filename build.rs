use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

const ASSETS: &[&str] = &[
    "bootstrap",
    "selection",
    "content-renderer",
    "content-enhancements",
    "content-navigation",
    "document-search",
    "directory-search",
    "content-controller",
    "memo",
    "fetch",
    "websocket",
    "sidebar",
];

const RERUN_PATHS: &[&str] = &[
    "src/template/assets/ts",
    "tsconfig.inline-js.json",
    "package.json",
    "package-lock.json",
    "scripts/build-inline-js.mjs",
];

fn main() {
    for path in RERUN_PATHS {
        println!("cargo:rerun-if-changed={path}");
    }

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR should be set by Cargo"));
    let inline_js_dir = out_dir.join("inline-js");
    fs::create_dir_all(&inline_js_dir).unwrap_or_else(|error| {
        panic!(
            "failed to create inline JS output directory {}: {error}",
            inline_js_dir.display()
        )
    });

    run_inline_js_build(&inline_js_dir);

    let generated_assets = validate_generated_assets(&inline_js_dir);
    write_manifest(&out_dir, &generated_assets);
}

fn run_inline_js_build(inline_js_dir: &Path) {
    let status = Command::new(npm_command())
        .args(["run", "build:inline-js"])
        .env("MV_INLINE_JS_OUT_DIR", inline_js_dir)
        .status()
        .unwrap_or_else(|error| {
            panic!(
                "failed to run npm inline JS build: {error}. Run `npm ci` then `npm run build:inline-js`."
            )
        });

    if !status.success() {
        panic!(
            "npm inline JS build failed with status {status}. Run `npm ci` then `npm run build:inline-js`."
        );
    }
}

fn npm_command() -> &'static str {
    if cfg!(windows) {
        "npm.cmd"
    } else {
        "npm"
    }
}

fn validate_generated_assets(inline_js_dir: &Path) -> Vec<PathBuf> {
    ASSETS
        .iter()
        .map(|asset| {
            let path = inline_js_dir.join(format!("{asset}.js"));
            if !path.is_file() {
                panic!(
                    "missing generated inline JS asset {}. Run `npm ci` then `npm run build:inline-js`.",
                    path.display()
                );
            }
            path
        })
        .collect()
}

fn write_manifest(out_dir: &Path, generated_assets: &[PathBuf]) {
    let mut manifest = String::from("const GENERATED_TEMPLATE: &str = concat!(\n");
    manifest.push_str("    \"(function() {\\n\",\n");

    for path in generated_assets {
        manifest.push_str("    include_str!(r#\"");
        manifest.push_str(&path.display().to_string());
        manifest.push_str("\"#),\n");
        manifest.push_str("    \"\\n\",\n");
    }

    manifest.push_str("    \"startMarkdownViewApp();\\n\",\n");
    manifest.push_str("    \"}());\\n\",\n");
    manifest.push_str(");\n");

    let manifest_path = out_dir.join("inline_script_manifest.rs");
    fs::write(&manifest_path, manifest).unwrap_or_else(|error| {
        panic!(
            "failed to write inline JS manifest {}: {error}",
            manifest_path.display()
        )
    });
}
