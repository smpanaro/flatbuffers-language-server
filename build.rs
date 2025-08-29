fn main() {
    let files = vec![
        "third_party/flatbuffers/src/util.cpp",
        "third_party/flatbuffers/src/idl_parser.cpp",
    ];

    cc::Build::new()
        .files(files)
        .include("third_party/flatbuffers/include")
        .cpp(true) // Treat files as C++
        .std("c++11")
        .compile("flatbuffers");

    println!("cargo:rerun-if-changed=build.rs");
}
