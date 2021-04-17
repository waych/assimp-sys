extern crate bindgen;
extern crate cmake;
extern crate pkg_config;
extern crate walkdir;

use cmake::Config;
use std::env;
use std::path::PathBuf;

/// Compiles the bundled code. Returns a list include_paths.
fn compile_bundled(out_path: &PathBuf) -> Vec<String> {
    // Compile assimp from source
    // Disable unnecessary stuff, it takes long enough to compile already
    let dst = Config::new("assimp")
        .define("ASSIMP_BUILD_ASSIMP_TOOLS", "OFF")
        .define("ASSIMP_BUILD_TESTS", "OFF")
        .define("ASSIMP_INSTALL_PDB", "OFF")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("LIBRARY_SUFFIX", "")
        .define("CMAKE_SUPPRESS_DEVELOPER_WARNINGS", "ON")
        // GCC doesn't work here, Assimp explicitly sets `-Werror` but
        // GCC emits some warnings that clang doesn't, setting `-Wno-error`
        // doesn't work because Assimp's cmake script adds `-Werror` _after_
        // our CFLAGS (even with `CMAKE_SUPPRESS_DEVELOPER_WARNINGS=ON`).
        //
        // When will C/C++ devs stop setting `-Werror` without a way to disable
        // it.
        .define("CMAKE_C_COMPILER", "clang")
        // For some reason, using `.pic(true)` doesn't work here, only
        // specifically setting it in CFLAGS
        .define("CMAKE_C_FLAGS", "-fPIC")
        .uses_cxx11()
        .build();

    let dst = dst.join("lib");
    println!("cargo:rustc-link-search=native={}", dst.display());

    // There's no way to extract this from `cmake::Config` so we have to emulate their
    // behaviour here (see the source for `cmake::Config::build`).
    let debug_postfix = match (
        &env::var("OPT_LEVEL").unwrap_or_default()[..],
        &env::var("PROFILE").unwrap_or_default()[..],
    ) {
        ("1", _) | ("2", _) | ("3", _) | ("s", _) | ("z", _) => "",
        ("0", _) => "d",
        (_, "debug") => "d",
        (_, _) => "",
    };

    println!("cargo:rustc-link-lib=static=assimp{}", debug_postfix);

    // Tell cargo we really want to rebuild if the main sources changed.
    for dirent in walkdir::WalkDir::new("assimp").min_depth(1) {
        let dirent = dirent.unwrap();
        let filename = dirent.file_name();
        let filename = filename.to_str().unwrap();
        if filename.ends_with(".h") || filename.ends_with(".cpp") || filename.ends_with(".inl") {
            println!("cargo:rerun-if-changed={}", dirent.path().to_str().unwrap());
        }
    }

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    vec![
        manifest_dir.join("assimp").join("include").into_os_string().into_string().unwrap(),
        out_path.join("include").into_os_string().into_string().unwrap(),
    ]
}

/// Find library and emit cargo: lines.  Returns include_paths
fn discover_library(out_path: &PathBuf) -> Vec<String> {
    let include_paths =
        if let Ok(assimp) = pkg_config::Config::new().exactly_version("5.0").probe("assimp") {
            for path in assimp.link_paths {
                println!("cargo:rustc-link-path={}", path.to_str().unwrap());
            }
            for lib in assimp.libs {
                println!("cargo:rustc-link-lib={}", lib);
            }

            assimp
                .include_paths
                .into_iter()
                .map(|p| p.into_os_string().into_string().unwrap())
                .collect::<Vec<_>>()
        } else {
            compile_bundled(out_path)
        };

    if let Ok(minizip) = pkg_config::probe_library("minizip") {
        for path in minizip.link_paths {
            println!("cargo:rustc-link-path={}", path.to_str().unwrap());
        }
        for lib in minizip.libs {
            println!("cargo:rustc-link-lib={}", lib);
        }
    }

    // Link to libstdc++ on GNU
    let target = env::var("TARGET").unwrap();
    if target.contains("gnu") {
        println!("cargo:rustc-link-lib=stdc++");
    } else if target.contains("apple") {
        println!("cargo:rustc-link-lib=c++");
    }
    include_paths
}

fn generate_bindings(out_path: &PathBuf, include_paths: &Vec<String>) {
    println!("cargo:rerun-if-changed=wrapper.h");
    let mut bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .blacklist_item("FP_ZERO")
        .blacklist_item("FP_SUBNORMAL")
        .blacklist_item("FP_NORMAL")
        .blacklist_item("FP_NAN")
        .blacklist_item("FP_INFINITE")
        .derive_partialeq(true)
        .derive_eq(true)
        .derive_hash(true)
        .derive_debug(true);

    for path in include_paths {
        bindings = bindings.clang_args(&["-I", &path]);
    }

    let bindings = bindings.generate().expect("Unable to generate bindings");

    let bindings_path = out_path.join("bindings.rs");
    bindings.write_to_file(&bindings_path).expect("Couldn't write bindings");
}

fn main() {
    let out_path = PathBuf::from(env::var_os("OUT_DIR").unwrap());

    let include_paths = discover_library(&out_path);
    generate_bindings(&out_path, &include_paths);
    println!("cargo:rerun-if-changed=build.rs");
}
