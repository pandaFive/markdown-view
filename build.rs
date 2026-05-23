use std::{
    collections::BTreeSet,
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
    if inline_js_dir.exists() {
        fs::remove_dir_all(&inline_js_dir).unwrap_or_else(|error| {
            panic!(
                "failed to remove stale inline JS output directory {}: {error}",
                inline_js_dir.display()
            )
        });
    }
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
    ensure_typescript_compiler();

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

fn ensure_typescript_compiler() {
    let tsc_path = Path::new("node_modules")
        .join("typescript")
        .join("bin")
        .join("tsc");
    if !tsc_path.is_file() {
        panic!(
            "TypeScript compiler was not found at {}. Run `npm ci` before running Cargo.",
            tsc_path.display()
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
    let expected: BTreeSet<String> = ASSETS.iter().map(|asset| format!("{asset}.js")).collect();
    let actual = generated_js_file_names(inline_js_dir);
    if actual != expected {
        let missing: Vec<_> = expected.difference(&actual).cloned().collect();
        let extra: Vec<_> = actual.difference(&expected).cloned().collect();
        panic!(
            "generated inline JS assets do not match ASSETS. missing: [{}], extra: [{}]",
            missing.join(", "),
            extra.join(", ")
        );
    }

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

fn generated_js_file_names(inline_js_dir: &Path) -> BTreeSet<String> {
    fs::read_dir(inline_js_dir)
        .unwrap_or_else(|error| {
            panic!(
                "failed to read generated inline JS output directory {}: {error}",
                inline_js_dir.display()
            )
        })
        .filter_map(|entry| {
            let entry = entry.unwrap_or_else(|error| {
                panic!(
                    "failed to read generated inline JS output entry in {}: {error}",
                    inline_js_dir.display()
                )
            });
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("js") {
                return None;
            }
            Some(
                path.file_name()
                    .and_then(|file_name| file_name.to_str())
                    .unwrap_or_else(|| {
                        panic!(
                            "generated inline JS asset path is not valid UTF-8: {}",
                            path.display()
                        )
                    })
                    .to_string(),
            )
        })
        .collect()
}

fn write_manifest(out_dir: &Path, generated_assets: &[PathBuf]) {
    let mut manifest = String::from("const GENERATED_TEMPLATE: &str = concat!(\n");

    for path in generated_assets {
        manifest.push_str("    include_str!(r#\"");
        manifest.push_str(&path.display().to_string());
        manifest.push_str("\"#),\n");
        manifest.push_str("    \"\\n\",\n");
    }

    manifest.push_str(");\n");

    let manifest_path = out_dir.join("inline_script_manifest.rs");
    fs::write(&manifest_path, manifest).unwrap_or_else(|error| {
        panic!(
            "failed to write inline JS manifest {}: {error}",
            manifest_path.display()
        )
    });
}
