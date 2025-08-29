use std::path::PathBuf;

fn main() {
    let files = vec![
        "third_party/flatbuffers/src/util.cpp",
        "third_party/flatbuffers/src/idl_parser.cpp",
        "src/cpp/wrapper.cpp",
    ];

    cc::Build::new()
        .files(files)
        .include("third_party/flatbuffers/include")
        .include("src/cpp")
        .cpp(true)
        .std("c++11")
        .compile("flatbuffers");

    println!("cargo:rerun-if-changed=src/cpp/wrapper.h");
    println!("cargo:rerun-if-changed=src/cpp/wrapper.cpp");

    let bindings = bindgen::Builder::default()
        .header("src/cpp/wrapper.h")
        // Tell cargo to invalidate the built crate whenever any of the
        // included header files changed.
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
