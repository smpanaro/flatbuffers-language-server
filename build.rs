use std::path::PathBuf;

fn main() {
    let files = vec![
        "third_party/flatbuffers/src/util.cpp",
        "src/cpp/patched_flatbuffers/idl_parser.cpp", // Use our patched version
        "src/cpp/wrapper.cpp",
    ];

    cc::Build::new()
        .files(files)
        // Add our patched include first so it's preferred.
        .include("src/cpp/patched_flatbuffers")
        .include("third_party/flatbuffers/include")
        .include("src/cpp")
        .cpp(true)
        .std("c++11")
        .compile("flatbuffers");

    println!("cargo:rerun-if-changed=src/cpp/wrapper.h");
    println!("cargo:rerun-if-changed=src/cpp/wrapper.cpp");
    println!("cargo:rerun-if-changed=src/cpp/patched_flatbuffers/idl.h");
    println!("cargo:rerun-if-changed=src/cpp/patched_flatbuffers/idl_parser.cpp");

    let bindings = bindgen::Builder::default()
        .header("src/cpp/wrapper.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
